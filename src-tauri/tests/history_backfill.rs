//! 价格历史后台补全接线测试（ADR-0122 / issue #1375）。
//!
//! **独立测试二进制**（不经 tests/commands 目标）：调度线程的单次拉起守卫是
//! 进程级单例，接线行为断言须在独立进程现场注入短时机（先例：
//! `tests/sync_trigger_poll.rs`，issue #959）。
//!
//! 断言全部对准用户可观察结果（CONTEXT-testing〈断言强度〉）：
//! - 启动接线所拉起的调度线程真实跑出补全：无历史标的的历史自己长出来
//!   （price_history 落行）、静默计数进度事件照发、实际写入发既有价格失效
//!   信号。本测试直接驱动启动入口（编排点是 `pub(crate)`，集成测试不可达，
//!   先例 `sync_trigger_poll.rs` 同形）；「删除 lib.rs 启动接线即变红」的
//!   负向判据由 `scripts/check-background-services.ts` 源码扫描守门承担
//!   （接线在测试不可直达的启动路径上以扫描守门替代，#959 / #961 先例）；
//! - 后台补全在途（门控桩阻塞在抓取点）时，前台同步命令照常完成、不被拒绝
//!   ——后台任务不占用「用户动作在途唯一」的槽位（ADR-0122 额度让路）。

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

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{Listener, Manager};

use ledger_infra::db::{self, DbState};
use ledger_infra::events;
use ledger_market_sync::{
    BackfillChannelsSlot, BulkFetchSurfaces, HISTORY_BACKFILL_PROGRESS, KlineBar, LaneTimings,
    QuoteItem, SyncFetchChannels, start_history_backfill_with,
};
use tauri_app_lib::commands::sync::{SyncChannelsSlot, sync_instrument_info};

/// 补全巡检周期（与下方注入的 `LaneTimings::window_poll` 同源）：同日窗口的
/// 观察窗按它取整周期推导，不另写时长字面量。
const POLL_INTERVAL: Duration = Duration::from_millis(300);

/// 门控桩的后台通道束：日 K 抓取点先通知「后台已在途」，再等测试放行并返回
/// 两个不同周的周线样本——price_history 的行只能来自后台补全（前台同步的
/// 日 K 桩返回空表，见下方前台桩）。抓取计数供「同日窗口不重跑」断言消费。
fn gated_backfill_channels(
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
    kline_calls: Arc<std::sync::atomic::AtomicUsize>,
) -> BackfillChannelsSlot {
    let channels = SyncFetchChannels {
        fetch_quotes: Box::new(|_| {
            Box::pin(async {
                unreachable!("后台补全不刷现价，批量报价通道不应被触达")
            })
        }),
        // 门控等待在闭包同步段完成（编排调用闭包即阻塞在途），应答装箱为 future
        // ——「后台补全真实在途」语义与断言不变（issue #1412 通道闭包 async 形态）。
        fetch_kline: Box::new(move |_query| {
            kline_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            entered.send(()).expect("在途通知应可送达");
            release
                .recv_timeout(Duration::from_secs(10))
                .expect("测试应放行后台抓取");
            Box::pin(async move {
                Ok(vec![
                    KlineBar::new("2026-01-05", 12.0),
                    KlineBar::new("2026-01-12", 13.0),
                ])
            })
        }),
        fetch_fx: Box::new(|_| Box::pin(async { Ok(vec![]) })),
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
    };
    BackfillChannelsSlot(Arc::new(tokio::sync::Mutex::new(channels)))
}

/// 前台同步命令的桩通道束：批量报价返回一条有效报价（现价照常落库），日 K
/// 返回空表——price_history 不因前台同步产生任何行，隔离出「历史只能来自
/// 后台补全」的断言面。
fn frontend_sync_channels() -> SyncChannelsSlot {
    let channels = SyncFetchChannels {
        fetch_quotes: Box::new(|_| {
            Box::pin(async move {
                Ok(vec![QuoteItem {
                    code: "600519".into(),
                    name: "贵州茅台".into(),
                    price_cents: Some(130_280),
                    price_date: None,
                }])
            })
        }),
        fetch_kline: Box::new(|_| Box::pin(async { Ok(vec![]) })),
        fetch_fx: Box::new(|_| Box::pin(async { Ok(vec![]) })),
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
    };
    SyncChannelsSlot(Arc::new(tokio::sync::Mutex::new(channels)))
}

/// 接线全流程：启动接线跑出后台补全（历史落行 + 静默计数 + 价格失效信号），
/// 且后台在途时前台同步不被拒绝。删掉 `start_background_services` 的启动接线
/// （或让后台占用前台槽位）本测试红。
#[test]
fn startup_wiring_backfills_history_and_frontend_sync_stays_unblocked() {
    // 提交点后置动作接线（置脏断言依赖）与交易域接缝接线（本位币读取钩子），
    // 与生产启动接线同形，幂等。
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();

    // 设备现场（sync_trigger_poll 同款）：mock 应用 + 独立临时目录文件库 +
    // 两扇门（调度线程做空转判定）。
    let dir = std::env::temp_dir().join(format!(
        "ledger-history-backfill-it-{}",
        ledger_infra::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(ledger_infra::db::encryption::EncryptionGate::new(false));
    app.manage(ledger_infra::db::boot::BootFailureGate::new());

    // 静态基线：一只沪市股票标的、无任何历史序列（首刷形态，队列必收）。
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().unwrap();
        tauri_app_lib::test_support::seed_instrument(
            &guard,
            "inst-1",
            "600519",
            "贵州茅台",
            "CNY",
            "sh",
        );
    }

    // 注入两车道桩束：后台门控（在途可控），前台独立桩（同步可独立完成）。
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let kline_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    app.manage(gated_backfill_channels(
        entered_tx,
        release_rx,
        kline_calls.clone(),
    ));
    app.manage(frontend_sync_channels());

    // 订阅两类用户可观察事件：静默计数进度与价格失效信号。
    let progress_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let log = progress_log.clone();
        app.listen(HISTORY_BACKFILL_PROGRESS, move |event| {
            log.lock().unwrap().push(event.payload().to_string());
        });
    }
    let price_signals: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    {
        let log = price_signals.clone();
        app.listen(events::PRICES_CHANGED, move |_| {
            *log.lock().unwrap() += 1;
        });
    }

    // 启动接线（生产唯一编排点的同款调用）：注入短时机（生产 30 秒延迟）。
    start_history_backfill_with(
        app.handle(),
        LaneTimings {
            startup_delay: Duration::from_millis(100),
            // 巡检周期取短：轮询在窗口内多次到期，同日不得重跑（下方断言）。
            // 观察窗按本常量取整周期推导，不另写时长字面量。
            window_poll: POLL_INTERVAL,
        },
    );

    // 等后台补全真实在途（门控抓取点）。
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("后台补全应到达日 K 抓取点");

    // 后台在途窗口内，前台同步命令照常完成、不被拒绝（不占用户动作在途槽位）。
    {
        let handle = app.handle().clone();
        let result = tauri::async_runtime::block_on(sync_instrument_info(
            handle.state::<DbState>(),
            handle.clone(),
        ));
        match result {
            Ok(result) => {
                assert_eq!(
                    result.synced, 1,
                    "前台同步照常刷现价（后台在途不拒绝前台动作）"
                );
            }
            Err(err) => panic!("后台在途时前台同步不应被拒绝：{err}"),
        }
    }

    // 放行后台抓取：补全完成，历史落行（只能来自后台——前台日 K 桩为空表）。
    release_tx.send(()).expect("放行应成功");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let rows: i64 = {
            let guard = conn.lock().unwrap();
            guard
                .query_row(
                    "SELECT count(*) FROM price_history WHERE instrument_id='inst-1'",
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        };
        if rows >= 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "启动接线未在限时内补齐历史（price_history 未落行）"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // 静默计数进度事件照发（唯一用户可见面），实际写入发既有价格失效信号。
    // 块作用域限定守卫：Mutex 不可重入，下方同日窗口断言要再次取同一锁。
    {
        let progress = progress_log.lock().unwrap();
        assert!(!progress.is_empty(), "后台补全应发出静默标的级计数进度事件");
        // 载荷为标的级 { done, total } 形状（结构断言，不做子串匹配）。
        let payloads: Vec<serde_json::Value> = progress
            .iter()
            .map(|payload| serde_json::from_str(payload).expect("进度载荷应为合法 JSON"))
            .collect();
        assert!(
            payloads.iter().any(|payload| {
                payload["total"] == serde_json::json!(1) && payload["done"].is_number()
            }),
            "进度载荷应为标的级 done/total 形状，实际 {payloads:?}"
        );
    }
    // 信号发射点在收尾裁决（数据落库 → 置脏 → 发射），**行可见不蕴含信号已
    // 送达**：后台补全的发射落在本轮收尾之后，与前台的发射之间天然有先后差。
    // 断言对准用户可观察契约「信号最终到达、不丢失」，故带超时等待（正常路径
    // 毫秒级即过；发射被删只剩单条时超时失败——负向判据不因等待而软化。先例：
    // `test_utils::GatedEmitter::wait_delivered` 的谓词等待与超时上界哲学）。
    let signal_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let delivered = *price_signals.lock().unwrap();
        if delivered >= 2 {
            break;
        }
        assert!(
            Instant::now() < signal_deadline,
            "前台同步与后台补全的实际写入各自发价格失效信号（成败同判的收尾裁决）：\
             限时内只到达 {delivered} 次"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // 同日窗口不重跑（AC「每个自然日窗口各跑一次」）：巡检多次到期后，日 K
    // 抓取仍只有首轮那一次，进度事件不再重新点亮（无第二轮的 { done: 0 }）。
    //
    // 规则本身（同日不开、跨日开）的权威在 `ledger_market_sync` 的单测
    // `daily_window_opens_only_once_per_beijing_calendar_day`；本断言守的是**接线**
    // ——调度循环确实把判定接上了「标记已跑 + 跑一轮」的副作用。观察窗取「两次
    // 巡检周期」而非固定睡眠：等待长度由此刻生效的注入周期决定，不靠猜；门被删
    // 或写坏时第二轮会在这两个周期内起跑，日 K 调用计数随即 >1（实测红）。
    let progress_now = progress_log.lock().unwrap().len();
    wait_poll_cycles(2, POLL_INTERVAL, || {
        kline_calls.load(std::sync::atomic::Ordering::SeqCst) == 1
            && progress_log.lock().unwrap().len() == progress_now
    });
    assert_eq!(
        kline_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "同一自然日窗口内巡检到期不得重跑第二轮"
    );
    assert_eq!(
        progress_log.lock().unwrap().len(),
        progress_now,
        "同日窗口内不产生新进度事件（done=0 的第二轮首帧不出现）"
    );
}

/// 等到若干次巡检周期过去、或 `still_silent` 变假（出现第二次动作）即返回。
fn wait_poll_cycles(cycles: u32, interval: Duration, still_silent: impl Fn() -> bool) {
    let step = interval / 4;
    let deadline = Instant::now() + interval * cycles;
    while Instant::now() < deadline {
        if !still_silent() {
            return;
        }
        std::thread::sleep(step);
    }
}
