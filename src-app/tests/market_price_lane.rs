//! 后台每日现价刷新接线测试·NSH 重生（issue #2079；根包 `tests/daily_price_sync.rs`
//! 的 NSH 等价形）。
//!
//! **独立测试二进制**（不经 app_headless 目标）：调度任务的单次拉起守卫是进程级
//! 单例，接线行为断言须在独立进程现场注入短时机与桩束（根包车道 IT 同形）。
//! NSH 启动接线（`run_startup` 成功分支单点）一并挂载三条车道——另两条车道在本
//! 进程注入静默桩束（补全空应答、汇率增量腿空应答），保证现价落行只能来自每日
//! 刷新（生产通道束不会被建、零真实网络）。
//!
//! 断言全部对准用户可观察结果（CONTEXT-testing〈断言强度〉）：
//! - 启动接线所拉起的调度任务真实跑出现价刷新：标的现价自己变新（market_prices
//!   落行）、实际写入经反馈桥投递（PriceWritten / PriceProgress 到达队列——引擎
//!   线程泵不在场，呈现面由 `market_lanes::probe` 与 lib 单测钉住）；
//! - 现价刷新不承担历史：现价落库而 price_history 零行（历史归后台补全）；
//! - 同一自然日窗口不重跑（「每自然日窗口一次」）。
//!
//! 根包版「后台在途时前台同步不被拒绝」断言在本壳不重演：前台同步的注入口是
//! 旧壳命令壳槽位（NSH 手动同步无测试注入口，#2046 交付口径），不占用户动作
//! 在途槽位的语义由域骨架单点承载（同一编排、无进程级在途互斥）。
//!
//! 「删除即红」：删掉 `run_startup` 成功分支的 `start_market_lanes()` 接线，本
//! 测试的抓取点等待即超时红（NSH 版负向判据由本测试承担——旧壳
//! `scripts/check-background-services.ts` 扫描面不含本壳接线）。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ledger_market_sync::{
    BulkFetchSurfaces, FxSyncChannels, LaneTimings, QuoteItem, SyncFetchChannels,
};
use rusqlite::{Connection, OptionalExtension};

// 本面只用目录与空库两件造数助手；其余助手随共享文件整载入（#[path] 双编译
// 形态，dead_code 局部放行——六件套测试面豁免语义同源，ADR-0060）。
#[allow(dead_code)]
#[path = "support/seed.rs"]
mod seed;

/// 每日刷新巡检周期（与注入的 `LaneTimings::window_poll` 同源）：同日窗口的
/// 观察窗按它取整周期推导，不另写时长字面量（根包 IT 同形）。
const POLL_INTERVAL: Duration = Duration::from_millis(300);

/// 种入一个有行情通道的股票标的（域薄皮 `create_instrument`，与根包
/// `test_support::seed_instrument` 同形：stock + 沪市通道码）。
fn seed_instrument(conn: &Connection, symbol: &str, name: &str) -> String {
    ledger_investment::create_instrument(
        conn,
        ledger_investment::InstrumentInput {
            symbol: symbol.to_owned(),
            kind: ledger_investment::InstrumentType::Stock,
            name: Some(name.to_owned()),
            currency_code: "CNY".to_owned(),
            market: Some("sh".to_owned()),
        },
    )
    .unwrap()
}

/// 门控桩的每日刷新通道束：批量报价抓取点先通知「后台已在途」，等测试放行并
/// 返回一条报价（1300.00 元）——market_prices 的新行只能来自后台每日刷新。
/// 抓取计数供「同日窗口不重跑」断言消费。
fn gated_daily_refresh_channels(
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
    quote_calls: Arc<AtomicUsize>,
) -> SyncFetchChannels {
    SyncFetchChannels {
        fetch_quotes: Box::new(move |_queries| {
            quote_calls.fetch_add(1, Ordering::SeqCst);
            entered.send(()).expect("在途通知应可送达");
            release
                .recv_timeout(Duration::from_secs(10))
                .expect("测试应放行后台抓取");
            Box::pin(async move {
                Ok(vec![QuoteItem {
                    code: "600519".into(),
                    name: "贵州茅台".into(),
                    price_cents: Some(130_000),
                    price_date: None,
                }])
            })
        }),
        fetch_kline: Box::new(|_| {
            Box::pin(async {
                unreachable!("现价刷新不发逐只 K 线请求（issue #1377）")
            })
        }),
        fetch_nav_history: Box::new(|_| {
            Box::pin(async {
                unreachable!("测试现场无基金标的，净值通道不应被触达")
            })
        }),
        fetch_fund_name: Box::new(|_| {
            Box::pin(async {
                unreachable!("测试现场无基金标的，名称通道不应被触达")
            })
        }),
        confirm_money_fund_form: Box::new(|_| Box::pin(async { Ok(false) })),
        bulk: BulkFetchSurfaces::absent(),
    }
}

/// 静默桩的价格历史补全通道束（同进程并挂载的第三条车道）：全通道空应答——
/// 补全零动作零网络（生产束不会被建）。
fn silent_backfill_channels() -> SyncFetchChannels {
    SyncFetchChannels {
        fetch_quotes: Box::new(|_| Box::pin(async { Ok(vec![]) })),
        fetch_kline: Box::new(|_| Box::pin(async { Ok(vec![]) })),
        fetch_nav_history: Box::new(|_| Box::pin(async { Ok(vec![]) })),
        fetch_fund_name: Box::new(|_| Box::pin(async { Ok(String::new()) })),
        confirm_money_fund_form: Box::new(|_| Box::pin(async { Ok(false) })),
        bulk: BulkFetchSurfaces::absent(),
    }
}

/// 静默桩的每日汇率通道束（同进程并挂载的第三条车道）：本位币单库深度已达 →
/// 增量腿空应答零落库；全量腿不应被触达。
fn silent_fx_channels() -> FxSyncChannels {
    FxSyncChannels {
        fetch_full: Box::new(|| {
            Box::pin(async { unreachable!("深度未达才走全量腿，本库无痕迹") })
        }),
        fetch_incremental: Box::new(|| Box::pin(async { Ok(vec![]) })),
    }
}

#[test]
fn startup_wiring_refreshes_prices_and_never_reruns_same_day_window() {
    // 车道测试现场（独立进程）：复位 → 武装 → 短时机 → 三车道桩束。
    nsh_app::market_lanes::reset_for_tests();
    nsh_app::market_lanes::enable_for_tests();
    nsh_app::market_lanes::set_test_timings(LaneTimings {
        startup_delay: Duration::from_millis(100),
        window_poll: POLL_INTERVAL,
    });

    // 测试账本：一只沪市股票标的，无现价、无历史序列。
    let dir = seed::make_book_dir("market-price-lane");
    seed::create_empty_book_file(&dir);
    let instrument_id = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.conn.lock().unwrap();
        seed_instrument(&conn, "600519", "贵州茅台")
    };

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let quote_calls = Arc::new(AtomicUsize::new(0));
    nsh_app::market_lanes::inject_price_refresh_channels_for_tests(gated_daily_refresh_channels(
        entered_tx,
        release_rx,
        quote_calls.clone(),
    ));
    nsh_app::market_lanes::inject_history_backfill_channels_for_tests(silent_backfill_channels());
    nsh_app::market_lanes::inject_fx_channels_for_tests(silent_fx_channels());

    // 启动接线（生产装配路径同款：run_startup 成功分支单点挂载三条车道）。
    nsh_app::app::set_book_dir(dir.to_path_buf());
    assert_eq!(
        nsh_app::app::run_startup(),
        nsh_app::app::StartupView::Ready,
        "引导成功呈现就绪态"
    );
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();

    // 等每日刷新真实在途（门控抓取点）。删 `start_market_lanes()` 接线本等待
    // 超时红（车道从未拉起，行情不更新）。
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("每日现价刷新应到达批量报价抓取点");

    // 放行后台抓取：每日刷新完成，现价被后台写入为 1300.00 元（130000 分）。
    release_tx.send(()).expect("放行应成功");

    // 事件驱动等待反馈桥投递（PriceWritten 在写提交后由车道线程收尾投递，
    // lane.rs 收尾单点——到达即写入已落盘，随后的库态断言无竞态；阻塞 recv
    // 由投递事件唤醒，不依赖测试线程轮询节奏，期限仅作兜底）。PriceProgress
    // 先于 PriceWritten 入队，忽略 BackfillProgress（静默桩车道可发）。
    let feedback_deadline = Instant::now() + Duration::from_secs(30);
    let (mut written, mut progressed) = (false, false);
    while !(written && progressed) {
        let remaining = feedback_deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_else(|| panic!("实际写入的反馈未在限时内到达（written={written}, progressed={progressed}）"));
        match nsh_app::market_lanes::recv_feedback_for_tests(remaining) {
            Some(nsh_app::market_lanes::LaneFeedback::PriceWritten) => written = true,
            Some(nsh_app::market_lanes::LaneFeedback::PriceProgress(_)) => progressed = true,
            Some(_) => {}
            None => panic!("实际写入的反馈未在限时内到达（written={written}, progressed={progressed}）"),
        }
    }

    // 现价已写入（PriceWritten 到达即写提交完成）：market_prices 落行 130000
    // 分，且现价刷新不承担历史——price_history 恒零行（历史归价格历史后台
    // 补全）。断言无轮询无竞态。
    let (price, history_rows): (Option<i64>, i64) = {
        let conn = db.conn.lock().unwrap();
        (
            conn.query_row(
                "SELECT price_cents FROM market_prices WHERE instrument_id = ?1",
                rusqlite::params![instrument_id],
                |r| r.get(0),
            )
            .optional()
            .unwrap(),
            conn.query_row(
                "SELECT count(*) FROM price_history WHERE instrument_id = ?1",
                rusqlite::params![instrument_id],
                |r| r.get(0),
            )
            .unwrap(),
        )
    };
    assert_eq!(price, Some(130_000), "启动接线刷出现价（market_prices 落行）");
    assert_eq!(
        history_rows, 0,
        "现价刷新不落任何历史点（含已有历史序列之外的单点）"
    );
    assert!(progressed, "现价刷新进度事件必达反馈桥");

    // 同日窗口不重跑（AC「每自然日窗口一次」）：巡检多次到期后，批量报价抓取
    // 仍只有首轮那一次（接线断言；规则权威在域单测 daily_window_opens_*）。
    let step = POLL_INTERVAL / 4;
    let deadline = Instant::now() + POLL_INTERVAL * 2;
    while Instant::now() < deadline {
        let calls = quote_calls.load(Ordering::SeqCst);
        if calls != 1 {
            panic!("同一自然日窗口内巡检到期不得重跑第二轮（实取 {calls} 次）");
        }
        std::thread::sleep(step);
    }
    assert_eq!(
        quote_calls.load(Ordering::SeqCst),
        1,
        "同一自然日窗口内巡检到期不得重跑第二轮"
    );
}
