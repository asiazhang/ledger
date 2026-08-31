//! 全量同步编排（issue #104 / #147）：分页循环取消与已落库数据保留、连接锁粒度、
//! SyncState 重入守卫与取消语义、终态进度，以及两同步命令共用的
//! 价格失效信号四终态语义（issue #236，ADR-0031 决策 2；判定单点收进 signals
//! 映射，ADR-0044 / issue #333）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::commands::sync::SyncState;
use crate::commands::sync::http::{MARKETS, MarketConfig, StockItem};
use crate::commands::sync::orchestrate::{
    ConnAccessor, GlobalConn, SyncOutcome, run_sync_pages, terminal_progress,
};
use crate::commands::sync::persist::build_existing_instruments;
use crate::error::Result;
use crate::models::SyncProgress;

use super::common::setup_db;

// ---------------------------------------------------------------------------
// 全量同步中断机制（issue #104）：分页循环每页检查取消标志、提前返回、已落库数据保留、
// 终态区分完成/中断；SyncState 的运行/取消标志语义。
// 锁粒度收窄（issue #147）：分页循环经连接访问器落库，拉取/推送进度不持锁。
// ---------------------------------------------------------------------------

/// 把内存库连接包进真实互斥锁，供分页循环经生产访问器驱动（与 DbState.conn 同构）。
fn locked_conn() -> (Arc<Mutex<Connection>>, GlobalConn) {
    let conn = Arc::new(Mutex::new(setup_db()));
    let accessor = GlobalConn(conn.clone());
    (conn, accessor)
}

/// 在互斥锁保护下查询一条标量（测试断言用）。
fn locked_scalar(conn: &Mutex<Connection>, sql: &str) -> i64 {
    let guard = conn.lock().unwrap();
    guard.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn run_sync_pages_cancelled_before_first_page_applies_nothing() {
    let (conn, accessor) = locked_conn();
    // 取消标志已置位：第一页起点即命中，提前返回，任何页都不该被拉取。
    let cancel = AtomicBool::new(true);
    let market_totals: Vec<(usize, &'static MarketConfig)> = vec![(250usize, &MARKETS[0])];

    let mut fetch_page = |_m: &MarketConfig, _p: usize| -> Result<Vec<StockItem>> {
        panic!("取消后不应再 fetch 任何分页");
    };
    let mut emitted: Vec<SyncProgress> = Vec::new();
    let outcome = run_sync_pages(
        &accessor,
        &cancel,
        &market_totals,
        250,
        &mut fetch_page,
        &mut |p| emitted.push(p),
    )
    .unwrap();

    assert_eq!(
        outcome,
        SyncOutcome::Cancelled {
            inserted: 0,
            updated: 0,
        }
    );
    assert!(emitted.is_empty(), "未处理任何页，不推送进度事件");
    let count = locked_scalar(&conn, "SELECT COUNT(*) FROM instruments");
    assert_eq!(count, 0);
}

#[test]
fn run_sync_pages_cancelled_midway_keeps_processed_data() {
    let (conn, accessor) = locked_conn();
    let cancel = Arc::new(AtomicBool::new(false));
    let market_totals: Vec<(usize, &'static MarketConfig)> = vec![(250usize, &MARKETS[0])];
    let grand_total = 250usize;

    let mut fetch_calls = 0usize;
    let cancel_clone = cancel.clone();
    let mut fetch_page = move |_market: &MarketConfig, _page: usize| -> Result<Vec<StockItem>> {
        fetch_calls += 1;
        // 第 2 次拉取后置位取消：第 2 页数据已落库，第 3 页命中取消而跳过。
        if fetch_calls == 2 {
            cancel_clone.store(true, Ordering::SeqCst);
        }
        let code = format!("{:06}", 600000 + fetch_calls);
        Ok(vec![StockItem {
            code: code.clone(),
            name: format!("名称-{code}"),
            price: Some(1000.0),
        }])
    };

    let mut emitted: Vec<SyncProgress> = Vec::new();
    let outcome = run_sync_pages(
        &accessor,
        &cancel,
        &market_totals,
        grand_total,
        &mut fetch_page,
        &mut |p| emitted.push(p),
    )
    .unwrap();

    // 中途取消：返回 Cancelled，统计 = 已处理的前 2 页（新增 2、更新 0）。
    assert_eq!(
        outcome,
        SyncOutcome::Cancelled {
            inserted: 2,
            updated: 0,
        }
    );
    // 已落库数据保留：前 2 页的标的与价格都在。
    let count = locked_scalar(&conn, "SELECT COUNT(*) FROM instruments");
    assert_eq!(count, 2);
    let price_count = locked_scalar(&conn, "SELECT COUNT(*) FROM market_prices");
    assert_eq!(price_count, 2);
    // 进度事件只推前两页（无终态；终态由 do_sync 层推送，此处聚焦分页循环）。
    assert_eq!(emitted.len(), 2);
    assert!(emitted.iter().all(|p| !p.done && !p.cancelled));
}

#[test]
fn run_sync_pages_completes_all_pages_when_not_cancelled() {
    let (conn, accessor) = locked_conn();
    let cancel = AtomicBool::new(false);

    // 150 只 → 2 页，不被取消：正常完成。
    let market_totals: Vec<(usize, &'static MarketConfig)> = vec![(150usize, &MARKETS[0])];
    let mut fetch_page = |_m: &MarketConfig, page: usize| -> Result<Vec<StockItem>> {
        let code = format!("{:06}", 600000 + page);
        Ok(vec![StockItem {
            code: code.clone(),
            name: format!("名称-{code}"),
            price: Some(1000.0),
        }])
    };
    let mut emitted: Vec<SyncProgress> = Vec::new();
    let outcome = run_sync_pages(
        &accessor,
        &cancel,
        &market_totals,
        150,
        &mut fetch_page,
        &mut |p| emitted.push(p),
    )
    .unwrap();

    assert_eq!(
        outcome,
        SyncOutcome::Completed {
            inserted: 2,
            updated: 0,
        }
    );
    assert_eq!(emitted.len(), 2);
    let count = locked_scalar(&conn, "SELECT COUNT(*) FROM instruments");
    assert_eq!(count, 2);
}

#[test]
fn global_conn_accessor_locks_and_releases_on_real_mutex() {
    // 生产访问器：持真实互斥锁执行落库闭包，返回后立即释放（try_lock 可再次获取）。
    let (conn, accessor) = locked_conn();
    let symbols = accessor
        .with_conn(|c| Ok(build_existing_instruments(c)?.len()))
        .unwrap();
    assert_eq!(symbols, 0);
    assert!(conn.try_lock().is_ok(), "with_conn 返回后必须已释放连接锁");
}

/// 生产访问器经连接层统一写入口落库（ADR-0032，#246 审计补齐）：落库成功在提交点
/// 置脏——行情同步写入也是账本数据变化，与其它写路径同一置脏语义。
#[test]
fn global_conn_with_conn_marks_dirty_via_write_entry() {
    let (conn, accessor) = locked_conn();
    accessor
        .with_conn(|c| {
            c.execute("CREATE TABLE seam_probe (x INTEGER)", [])
                .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
            Ok(())
        })
        .expect("落库成功");
    let state = crate::auto_backup::get_state(&conn.lock().unwrap()).expect("读状态");
    assert!(state.dirty, "with_conn 落库成功应置脏");
}

/// 落库闭包失败 → 不置脏（写入口闭包失败语义），错误原样上抛。
#[test]
fn global_conn_with_conn_error_does_not_mark_dirty() {
    let (conn, accessor) = locked_conn();
    let err = accessor
        .with_conn(|_c| Err::<(), _>(crate::error::AppError::Invalid("boom".into())))
        .unwrap_err();
    assert!(err.to_string().contains("boom"));
    let state = crate::auto_backup::get_state(&conn.lock().unwrap()).expect("读状态");
    assert!(!state.dirty, "落库失败不应置脏");
}

#[test]
fn run_sync_pages_locks_only_for_per_page_persist() {
    // 锁时间线：拉取期间锁可用（同步不持锁）；每页只在落库时短暂加锁、释放后才推进度。
    let (conn, accessor) = locked_conn();
    let cancel = AtomicBool::new(false);

    // 日志访问器：在真实落库前后记录加锁/释放事件。
    struct Logging {
        inner: GlobalConn,
        events: Arc<Mutex<Vec<&'static str>>>,
    }
    let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let logging = Logging {
        inner: accessor,
        events: events.clone(),
    };
    impl ConnAccessor for Logging {
        fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
            self.events.lock().unwrap().push("lock");
            let r = self.inner.with_conn(f);
            self.events.lock().unwrap().push("unlock");
            r
        }
    }

    let fetch_events = events.clone();
    let fetch_conn = conn.clone();
    let mut fetch_page = move |_m: &MarketConfig, page: usize| -> Result<Vec<StockItem>> {
        fetch_events.lock().unwrap().push("fetch");
        // 拉取期间连接锁必须可用：同步任务不得在网络等待时持锁（issue #147）。
        assert!(fetch_conn.try_lock().is_ok(), "拉取分页期间不得持有连接锁");
        let code = format!("{:06}", 600000 + page);
        Ok(vec![StockItem {
            code: code.clone(),
            name: format!("名称-{code}"),
            price: Some(1000.0),
        }])
    };

    let emit_events = events.clone();
    let mut emit = move |_p: SyncProgress| {
        emit_events.lock().unwrap().push("emit");
    };

    // 2 页：预期每页时间线 fetch → lock → unlock → emit。
    let market_totals: Vec<(usize, &'static MarketConfig)> = vec![(150usize, &MARKETS[0])];
    let outcome = run_sync_pages(
        &logging,
        &cancel,
        &market_totals,
        150,
        &mut fetch_page,
        &mut emit,
    )
    .unwrap();
    assert_eq!(
        outcome,
        SyncOutcome::Completed {
            inserted: 2,
            updated: 0,
        }
    );

    let timeline = events.lock().unwrap().clone();
    assert_eq!(
        timeline,
        vec![
            "fetch", "lock", "unlock", "emit", "fetch", "lock", "unlock", "emit"
        ],
        "每页：锁外拉取 → 短暂持锁落库 → 释放 → 锁外推进度"
    );
    assert_eq!(locked_scalar(&conn, "SELECT COUNT(*) FROM instruments"), 2);
}

#[test]
fn sync_state_try_start_guards_reentry_and_clears_cancel() {
    let state = SyncState::default();
    // 初始：无同步在跑 → 取消命令应表现为「无副作用」。
    assert!(!state.is_running());
    assert!(!state.is_cancel_requested());

    // 首次启动成功：标记运行中、清除取消标志。
    assert!(state.try_start());
    assert!(state.is_running());
    assert!(!state.is_cancel_requested());

    // 已置位取消标志（后台线程收尾前）。
    state.request_cancel();
    assert!(state.is_cancel_requested());

    // 再次启动被拒：guard 阻止重入，不清掉已置位的取消标志（前一次同步得以继续被中断）。
    assert!(!state.try_start());
    assert!(state.is_running());
    assert!(state.is_cancel_requested());
}

#[test]
fn sync_state_cancel_distinguishes_running_and_idle() {
    let state = SyncState::default();
    // 无同步在跑：无副作用、返回明确提示。
    let idle = state.cancel();
    assert!(!idle.cancelled);
    assert_eq!(idle.message, "当前没有正在进行的同步");
    assert!(!state.is_cancel_requested(), "无同步时取消不应置位取消标志");

    // 有同步在跑：置位取消标志、返回中断提示。
    assert!(state.try_start());
    let running = state.cancel();
    assert!(running.cancelled);
    assert_eq!(running.message, "已请求中断同步");
    assert!(state.is_cancel_requested());
}

#[test]
fn terminal_progress_distinguishes_completed_and_cancelled() {
    // 完成终态：done=true、cancelled=false、计数正确。
    let completed = terminal_progress(&SyncOutcome::Completed {
        inserted: 3,
        updated: 1,
    });
    assert!(completed.done);
    assert!(!completed.cancelled);
    assert_eq!(completed.total_inserted, 3);
    assert_eq!(completed.total_updated, 1);

    // 中断终态：done=true、cancelled=true、计数为已处理部分。
    let cancelled = terminal_progress(&SyncOutcome::Cancelled {
        inserted: 2,
        updated: 0,
    });
    assert!(cancelled.done);
    assert!(cancelled.cancelled);
    assert_eq!(cancelled.total_inserted, 2);
    assert_eq!(cancelled.total_updated, 0);
}

// ---------------------------------------------------------------------------
// 价格失效信号（issue #236，ADR-0031 决策 2 → #333 判定归一化 ADR-0044）：
// 「是否发」单点收进 signals 映射（signals_for），壳层只把终态归一化为证据——
// 到达保留落库的终态（成功/用户中断）按实际写入 n>0 归一化为 PriceWritten，
// 失败无 Ok 终态、无证据零信号。四终态语义在此钉住，语义与迁移前一致。
// ---------------------------------------------------------------------------

#[test]
fn prices_changed_signal_pins_four_terminal_states() {
    use crate::signals::{Signal, WriteEvidence as E, WriteOp as Op, signals_for};

    // 终态 → 证据归一化（两同步命令壳层同式）：Some(n) 为到达保留落库的终态
    // （成功或用户中断）且实际写入 n 条；None 为失败，无证据。
    fn evidence(written: Option<usize>) -> E {
        written.map_or(E::None, |n| E::PriceWritten(n > 0))
    }
    fn assert_signals(actual: &[Signal], expected: &[Signal]) {
        assert_eq!(actual, expected);
    }

    // 成功且有落库：发。
    assert_signals(
        signals_for(Op::SyncHoldingPrices, evidence(Some(3))),
        &[Signal::PricesChanged],
    );
    // 零更新（无持仓/全部跳过，落库 0）：库内零变化，不发。
    assert_signals(signals_for(Op::SyncHoldingPrices, evidence(Some(0))), &[]);
    // 用户中断但有落库（中断保留已落库价格，不发信号即失真）：发（全量同步同一映射行）。
    assert_signals(
        signals_for(Op::SyncInstruments, evidence(Some(120))),
        &[Signal::PricesChanged],
    );
    // 失败且无落库：不发。
    assert_signals(signals_for(Op::SyncInstruments, evidence(None)), &[]);
}
