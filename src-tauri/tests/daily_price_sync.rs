//! 后台每日现价刷新接线测试（ADR-0122 决策 3 / issue #1377）。
//!
//! **独立测试二进制**（不经 tests/commands 目标）：调度线程的单次拉起守卫是
//! 进程级单例，接线行为断言须在独立进程现场注入短时机（先例：
//! `tests/history_backfill.rs`，issue #1375 同形）。
//!
//! 断言全部对准用户可观察结果（CONTEXT-testing〈断言强度〉）：
//! - 启动接线所拉起的调度线程真实跑出现价刷新：标的现价自己变新（market_prices
//!   落行）、实际写入发既有价格失效信号；本测试直接驱动启动入口（「删除 lib.rs
//!   启动接线即变红」的负向判据由 `scripts/check-background-services.ts` 源码
//!   扫描守门承担——接线在测试不可直达的启动路径上以扫描守门替代，#959 / #961
//!   先例）；
//! - 后台每日刷新在途（门控桩阻塞在抓取点）时，前台同步命令照常完成、不被拒绝
//!   ——后台形态不占用「用户动作在途唯一」的槽位（ADR-0122 决策 6）；
//! - 现价刷新不承担历史：现价落库而 price_history 零行（历史归后台补全）；
//! - 同一自然日窗口不重跑（「每自然日窗口一次」）。

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
    BulkFetchSurfaces, DailyPriceRefreshChannelsSlot, DailyPriceRefreshTimings, StockItem,
    SyncFetchChannels, start_daily_price_refresh_with,
};
use tauri_app_lib::commands::sync::{SyncChannelsSlot, sync_instrument_info};

/// 门控桩的后台通道束：批量报价抓取点先通知「后台已在途」，再等测试放行并返回
/// 一条报价（现价 1300.00 元）——market_prices 的新行只能来自后台每日刷新（前台
/// 桩写的是另一个价，见下方前台桩）。抓取计数供「同日窗口不重跑」断言消费。
fn gated_daily_refresh_channels(
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
    ulist_calls: Arc<std::sync::atomic::AtomicUsize>,
) -> DailyPriceRefreshChannelsSlot {
    let channels = SyncFetchChannels {
        fetch_ulist: Box::new(move |_secids| {
            ulist_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            entered.send(()).expect("在途通知应可送达");
            release
                .recv_timeout(Duration::from_secs(10))
                .expect("测试应放行后台抓取");
            Ok(vec![StockItem {
                code: "600519".into(),
                name: "贵州茅台".into(),
                price: Some(1300.0),
                precision: None,
            }])
        }),
        fetch_kline: Box::new(|_| unreachable!("现价刷新不发逐只日 K 请求（issue #1377）")),
        fetch_fx: Box::new(|_| Ok(vec![])),
        fetch_nav: Box::new(|_| unreachable!("测试现场无基金标的，净值通道不应被触达")),
        fetch_nav_full: Box::new(|_| unreachable!("测试现场无基金标的，全量净值通道不应被触达")),
        fetch_fund_name: Box::new(|_| unreachable!("测试现场无基金标的，名称通道不应被触达")),
        bulk: BulkFetchSurfaces::absent(),
    };
    DailyPriceRefreshChannelsSlot(Arc::new(Mutex::new(channels)))
}

/// 前台同步命令的桩通道束：批量报价返回与后台桩**不同**的报价（1302.80 元），
/// 区分「哪一轮写了哪个价」。
fn frontend_sync_channels() -> SyncChannelsSlot {
    let channels = SyncFetchChannels {
        fetch_ulist: Box::new(|_| {
            Ok(vec![StockItem {
                code: "600519".into(),
                name: "贵州茅台".into(),
                price: Some(1302.80),
                precision: None,
            }])
        }),
        fetch_kline: Box::new(|_| unreachable!("现价刷新不发逐只日 K 请求（issue #1377）")),
        fetch_fx: Box::new(|_| Ok(vec![])),
        fetch_nav: Box::new(|_| unreachable!("测试现场无基金标的，净值通道不应被触达")),
        fetch_nav_full: Box::new(|_| unreachable!("测试现场无基金标的，全量净值通道不应被触达")),
        fetch_fund_name: Box::new(|_| unreachable!("测试现场无基金标的，名称通道不应被触达")),
        bulk: BulkFetchSurfaces::absent(),
    };
    SyncChannelsSlot(Arc::new(Mutex::new(channels)))
}

/// 接线全流程：启动接线跑出每日现价刷新（现价落行 + 价格失效信号），且后台在
/// 途时前台同步不被拒绝；现价刷新不落历史；同日窗口不重跑。删掉
/// `start_background_services` 的 `start_daily_price_refresh` 接线（或让后台
/// 占用前台槽位）本测试红。
#[test]
fn startup_wiring_refreshes_prices_and_frontend_sync_stays_unblocked() {
    // 提交点后置动作接线（置脏断言依赖）与交易域接缝接线（本位币读取钩子），
    // 与生产启动接线同形，幂等。
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();

    // 设备现场（history_backfill IT 同款）：mock 应用 + 独立临时目录文件库 +
    // 两扇门（调度线程做空转判定）。
    let dir = std::env::temp_dir().join(format!(
        "ledger-daily-price-refresh-it-{}",
        ledger_infra::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(ledger_infra::db::encryption::EncryptionGate::new(false));
    app.manage(ledger_infra::db::boot::BootFailureGate::new());

    // 静态基线：一只沪市股票标的，无现价、无历史序列。
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
    let ulist_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    app.manage(gated_daily_refresh_channels(
        entered_tx,
        release_rx,
        ulist_calls.clone(),
    ));
    app.manage(frontend_sync_channels());

    // 订阅用户可观察事件：价格失效信号。
    let price_signals: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    {
        let log = price_signals.clone();
        app.listen(events::PRICES_CHANGED, move |_| {
            *log.lock().unwrap() += 1;
        });
    }

    // 启动接线（生产唯一编排点的同款调用）：注入短时机（生产 30 秒延迟）。
    start_daily_price_refresh_with(
        app.handle(),
        DailyPriceRefreshTimings {
            startup_delay: Duration::from_millis(100),
            // 巡检周期取短：轮询在窗口内多次到期，同日不得重跑（下方断言）。
            window_poll: Duration::from_millis(300),
        },
    );

    // 等每日刷新真实在途（门控抓取点）。
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("每日现价刷新应到达批量报价抓取点");

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

    // 放行后台抓取：每日刷新完成，现价被后台写入覆盖为 1300.00 元（130000 分）。
    release_tx.send(()).expect("放行应成功");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let price: Option<i64> = {
            let guard = conn.lock().unwrap();
            guard
                .query_row(
                    "SELECT price_cents FROM market_prices WHERE instrument_id='inst-1'",
                    [],
                    |r| r.get(0),
                )
                .ok()
        };
        if price == Some(130_000) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "启动接线未在限时内刷出现价（market_prices 未被后台写入覆盖）"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // 现价刷新不承担历史：price_history 恒零行（历史归价格历史后台补全）。
    let history_rows: i64 = {
        let guard = conn.lock().unwrap();
        guard
            .query_row(
                "SELECT count(*) FROM price_history WHERE instrument_id='inst-1'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(
        history_rows, 0,
        "现价刷新不落任何历史点（含已有历史序列之外的单点）"
    );

    // 实际写入各自发既有价格失效信号（前台写入 + 后台写入，成败同判的收尾裁决）。
    // 信号发射点在收尾裁决（数据落库 → 置脏 → 发射），**现价可见不蕴含信号已
    // 送达**：后台刷新这一轮的发射落在收尾之后，与前台的发射之间天然有先后差。
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
            "前台同步与后台每日刷新的实际写入各自发价格失效信号：限时内只到达 {delivered} 次"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // 同日窗口不重跑（AC「每自然日窗口一次」）：巡检多次到期后，批量报价抓取
    // 仍只有首轮那一次。
    std::thread::sleep(Duration::from_millis(1_000));
    assert_eq!(
        ulist_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "同一自然日窗口内巡检到期不得重跑第二轮"
    );
}
