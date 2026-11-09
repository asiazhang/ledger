//! 价格历史后台补全接线测试·NSH 重生（issue #2079；根包
//! `tests/history_backfill.rs` 的 NSH 等价形）。
//!
//! **独立测试二进制**：调度任务的单次拉起守卫是进程级单例，接线行为断言须在
//! 独立进程现场注入短时机与桩束。NSH 启动接线一并挂载三条车道——每日刷新车道
//! 注入空报价桩（现价零写入，历史行的来路唯一）、汇率车道注入增量腿空应答桩
//!（本位币单库深度已达，零落库零网络），生产通道束不会被建、零真实网络。
//!
//! 断言全部对准用户可观察结果（CONTEXT-testing〈断言强度〉）：
//! - 启动接线所拉起的调度任务真实跑出补全：无历史标的的历史自己长出来
//!   （price_history 落行）、静默计数进度经反馈桥投递（BackfillProgress 到达
//!   队列）、实际写入经反馈桥投递（PriceWritten 到达队列）；
//! - 同一自然日窗口不重跑（「每自然日窗口一次」）。
//!
//! 根包版「后台在途时前台同步不被拒绝」断言在本壳不重演（前台同步注入口是旧
//! 壳命令壳槽位，见 market_price_lane.rs 头注同款裁决）。
//!
//! 「删除即红」：删掉 `run_startup` 成功分支的 `start_market_lanes()` 接线，
//! 本测试的抓取点等待即超时红。

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
    BulkFetchSurfaces, FxSyncChannels, KlineBar, LaneTimings, SyncFetchChannels,
};
use rusqlite::Connection;

// 本面只用目录与空库两件造数助手；其余助手随共享文件整载入（#[path] 双编译
// 形态，dead_code 局部放行——六件套测试面豁免语义同源，ADR-0060）。
#[allow(dead_code)]
#[path = "support/seed.rs"]
mod seed;

/// 补全巡检周期（与注入的 `LaneTimings::window_poll` 同源）：同日窗口的观察窗
/// 按它取整周期推导，不另写时长字面量。
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

/// 门控桩的补全通道束：周 K 抓取点先通知「后台已在途」，等测试放行并返回两个
/// 不同周的周线样本——price_history 的行只能来自后台补全（每日刷新桩的报价为
/// 空表，见下方静默桩）。抓取计数供「同日窗口不重跑」断言消费。
fn gated_backfill_channels(
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
    kline_calls: Arc<AtomicUsize>,
) -> SyncFetchChannels {
    SyncFetchChannels {
        fetch_quotes: Box::new(|_| {
            Box::pin(async {
                unreachable!("后台补全不刷现价，批量报价通道不应被触达")
            })
        }),
        fetch_kline: Box::new(move |_query| {
            kline_calls.fetch_add(1, Ordering::SeqCst);
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

/// 静默桩的每日现价刷新通道束：报价空表——现价零写入，历史行的来路唯一。
fn silent_refresh_channels() -> SyncFetchChannels {
    SyncFetchChannels {
        fetch_quotes: Box::new(|_| Box::pin(async { Ok(vec![]) })),
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

/// 静默桩的每日汇率通道束：本位币单库深度已达 → 增量腿空应答零落库。
fn silent_fx_channels() -> FxSyncChannels {
    FxSyncChannels {
        fetch_full: Box::new(|| {
            Box::pin(async { unreachable!("深度未达才走全量腿，本库无痕迹") })
        }),
        fetch_incremental: Box::new(|| Box::pin(async { Ok(vec![]) })),
    }
}

#[test]
fn startup_wiring_backfills_history_and_never_reruns_same_day_window() {
    // 车道测试现场（独立进程）：复位 → 武装 → 短时机 → 三车道桩束。
    nsh_app::market_lanes::reset_for_tests();
    nsh_app::market_lanes::enable_for_tests();
    nsh_app::market_lanes::set_test_timings(LaneTimings {
        startup_delay: Duration::from_millis(100),
        window_poll: POLL_INTERVAL,
    });

    // 测试账本：一只沪市股票标的、无任何历史序列（首刷形态，队列必收）。
    let dir = seed::make_book_dir("market-backfill-lane");
    seed::create_empty_book_file(&dir);
    let instrument_id = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.conn.lock().unwrap();
        seed_instrument(&conn, "600519", "贵州茅台")
    };

    // 注入桩束：补全门控（在途可控），刷新静默（历史行来路唯一），汇率静默。
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let kline_calls = Arc::new(AtomicUsize::new(0));
    nsh_app::market_lanes::inject_history_backfill_channels_for_tests(gated_backfill_channels(
        entered_tx,
        release_rx,
        kline_calls.clone(),
    ));
    nsh_app::market_lanes::inject_price_refresh_channels_for_tests(silent_refresh_channels());
    nsh_app::market_lanes::inject_fx_channels_for_tests(silent_fx_channels());

    // 启动接线（生产装配路径同款：run_startup 成功分支单点挂载三条车道）。
    nsh_app::app::set_book_dir(dir.clone());
    assert_eq!(
        nsh_app::app::run_startup(),
        nsh_app::app::StartupView::Ready,
        "引导成功呈现就绪态"
    );
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();

    // 等后台补全真实在途（门控抓取点）。删 `start_market_lanes()` 接线本等待
    // 超时红（车道从未拉起，历史不生长）。
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("后台补全应到达周 K 抓取点");

    // 放行后台抓取：补全完成，历史落行（只能来自后台——每日刷新桩的报价为空表）。
    release_tx.send(()).expect("放行应成功");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let rows: i64 = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT count(*) FROM price_history WHERE instrument_id = ?1",
                rusqlite::params![instrument_id],
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

    // 现价零写入：market_prices 恒零行（补全不刷现价，刷新桩报价空表）。
    let price_rows: i64 = {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT count(*) FROM market_prices WHERE instrument_id = ?1",
            rusqlite::params![instrument_id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(price_rows, 0, "补全不落现价（历史行的来路唯一）");

    // 实际写入经反馈桥投递（成败同判收尾裁决的投递半边）：PriceWritten 与静默
    // 计数（BackfillProgress，含 total 预告）都必达。泵不在场（本二进制无引
    // 擎），队列即排空断言面；呈现半边由 market_lanes::probe 钉住。
    let feedback_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let drained = nsh_app::market_lanes::take_feedback_for_tests();
        let written = drained
            .iter()
            .any(|fb| matches!(fb, nsh_app::market_lanes::LaneFeedback::PriceWritten));
        let counted = drained
            .iter()
            .any(|fb| matches!(fb, nsh_app::market_lanes::LaneFeedback::BackfillProgress(_)));
        if written && counted {
            break;
        }
        assert!(
            Instant::now() < feedback_deadline,
            "实际写入 / 静默计数的反馈未在限时内到达（written={written}, counted={counted}）"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // 同日窗口不重跑（AC「每自然日窗口一次」）：巡检多次到期后，周 K 抓取仍
    // 只有首轮那一次（接线断言；规则权威在域单测 daily_window_opens_*）。
    let step = POLL_INTERVAL / 4;
    let deadline = Instant::now() + POLL_INTERVAL * 2;
    while Instant::now() < deadline {
        let calls = kline_calls.load(Ordering::SeqCst);
        if calls != 1 {
            panic!("同一自然日窗口内巡检到期不得重跑第二轮（实取 {calls} 次）");
        }
        std::thread::sleep(step);
    }
    assert_eq!(
        kline_calls.load(Ordering::SeqCst),
        1,
        "同一自然日窗口内巡检到期不得重跑第二轮"
    );
}
