//! 后台每日汇率增量同步接线测试·NSH 重生（issue #2079；根包
//! `tests/daily_fx_sync.rs` 的 NSH 等价形）。
//!
//! **独立测试二进制**：调度任务的单次拉起守卫是进程级单例，接线行为断言须在
//! 独立进程现场注入短时机与桩束。NSH 启动接线一并挂载三条车道——另两条车道在
//! 本进程注入静默桩束（本库无行情标的，取数通道零触达，生产通道束不会被建、
//! 零真实网络）。
//!
//! 断言全部对准用户可观察结果（CONTEXT-testing〈断言强度〉）：
//! - 启动接线真实拉起每日汇率通道：账本出现非本位币痕迹后，一轮同步把汇率
//!   历史与当期汇率自己写进库（fx_rate_history / exchange_rates 落行）；实际
//!   落库全部来自每日通道（并发一路被互斥拒绝、零写入）；
//! - 每日通道在途（门控桩阻塞在取数点）时，另一路汇率同步（独立通道束 +
//!   独立会话，另一台设备 / 手动入口并发的进程内同形，共用进程级单例门）立即
//!   被互斥拒绝——报 `fx.sync-in-progress` 码化错误，不排队等待、不触达取数、
//!   零落库（issue #1762 收窄 issue #1546 AC4）；
//! - 同一自然日窗口不重跑（「每自然日至多一次」，issue #1546 AC2）。
//!
//! 失败面的行为断言归域单测（三态错误分类 / 单事务原子 / 人工行保护）；每日
//! 车道无反馈（可重建缓存，无置脏无失效信号）——反馈桥零件即本车道裁决面。
//!
//! 「删除即红」：删掉 `run_startup` 成功分支的 `start_market_lanes()` 接线，
//! 本测试的取数点等待即超时红。

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

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use ledger_accounts::{AccountInput, AccountType};
use ledger_market_sync::{
    BulkFetchSurfaces, EcbDayRates, FX_SYNC_GATE, FacadeWriteSession, FxSyncChannels, LaneTimings,
    SyncFetchChannels, sync_fx_rates_guarded,
};
use rusqlite::{Connection, OptionalExtension};

// 本面只用目录与空库两件造数助手；其余助手随共享文件整载入（#[path] 双编译
// 形态，dead_code 局部放行——六件套测试面豁免语义同源，ADR-0060）。
#[allow(dead_code)]
#[path = "support/seed.rs"]
mod seed;

/// 每日汇率通道巡检周期（与注入的 `LaneTimings::window_poll` 同源）：同日窗口
/// 的观察窗按它取整周期推导，不另写时长字面量。
const POLL_INTERVAL: Duration = Duration::from_millis(300);

/// 字典币种（种子库全量，本位币 CNY 在内）——通道桩按字典全量腿构造日快照
///（编排按字典全量对本位币推导，缺腿的币种对无点；根包 IT 同形）。
fn dictionary_codes(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT code FROM currencies ORDER BY code")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

/// 一天快照：腿覆盖字典全部币种（根包 IT 同一腿公式）。
fn day(conn: &Connection, date: &str, cny_leg: f64) -> EcbDayRates {
    let codes = dictionary_codes(conn);
    let rates = codes
        .iter()
        .map(|code| {
            let leg = match code.as_str() {
                "CNY" => cny_leg,
                "EUR" => 0.9,
                other => 1.0 + (codes.iter().position(|c| c == other).unwrap() as f64) / 10.0,
            };
            (code.clone(), leg)
        })
        .collect::<BTreeMap<_, _>>();
    EcbDayRates {
        date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        rates,
    }
}

/// USD 腿取值（当期汇率交叉值的期望计算用，与 `day` 的腿公式同源）。
fn usd_leg(codes: &[String]) -> f64 {
    let idx = codes.iter().position(|c| c == "USD").unwrap();
    1.0 + (idx as f64) / 10.0
}

/// 后台桩的四天快照（夹具最早周 / 痕迹日当周 / 近期两周）：覆盖区间与并发桩
///（三天快照）重叠三周、多出一周——放行后多出的那一周与当期汇率的改写是
/// 「后台通道真实落库」的可观察面。
const LANE_DAY_SPECS: &[(&str, f64)] = &[
    ("2022-06-06", 7.2),
    ("2022-06-15", 7.3),
    ("2026-09-14", 7.4),
    ("2026-09-21", 7.5),
];

/// 并发一路的三天快照（与后台桩重叠三周）。
const OTHER_DAY_SPECS: &[(&str, f64)] = &[
    ("2022-06-06", 7.2),
    ("2022-06-15", 7.3),
    ("2026-09-14", 7.4),
];

/// 桩通道束构造单点：快照腿覆盖字典全量币种、全量腿按 `gated` 门控或直通并
/// 各自计数（供「同日窗口不重跑」与「并发一路只取数一次」断言消费）、增量腿
/// 在深度未达的首轮不该被触达（unreachable 兜底）。
fn stub_fx_channels(
    conn: &Connection,
    specs: &[(&str, f64)],
    full_calls: Arc<AtomicUsize>,
    gated: Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
) -> FxSyncChannels {
    let days: Vec<EcbDayRates> = specs.iter().map(|(d, cny)| day(conn, d, *cny)).collect();
    FxSyncChannels {
        fetch_full: Box::new(move || {
            full_calls.fetch_add(1, Ordering::SeqCst);
            if let Some((entered, release)) = &gated {
                entered.send(()).expect("在途通知应可送达");
                release
                    .recv_timeout(Duration::from_secs(10))
                    .expect("测试应放行后台取数");
            }
            let days = days.clone();
            Box::pin(async move { Ok(days) })
        }),
        fetch_incremental: Box::new(|| {
            Box::pin(async {
                unreachable!("深度未达的首轮走全量腿，增量腿不应被触达")
            })
        }),
    }
}

/// 静默桩的每日现价刷新通道束（同进程并挂载的另两条车道）：本库无行情标的，
/// 通道零触达；空应答兜底防真实网络。
fn silent_price_channels() -> SyncFetchChannels {
    SyncFetchChannels {
        fetch_quotes: Box::new(|_| Box::pin(async { Ok(vec![]) })),
        fetch_kline: Box::new(|_| Box::pin(async { Ok(vec![]) })),
        fetch_nav_history: Box::new(|_| Box::pin(async { Ok(vec![]) })),
        fetch_fund_name: Box::new(|_| Box::pin(async { Ok(String::new()) })),
        confirm_money_fund_form: Box::new(|_| Box::pin(async { Ok(false) })),
        bulk: BulkFetchSurfaces::absent(),
    }
}

/// 汇率历史总行数（落库面的可观察合计）。
fn fx_point_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM fx_rate_history", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn startup_wiring_syncs_fx_rates_and_concurrent_sync_is_rejected() {
    // 车道测试现场（独立进程）：复位 → 武装 → 短时机 → 三车道桩束。
    nsh_app::market_lanes::reset_for_tests();
    nsh_app::market_lanes::enable_for_tests();
    nsh_app::market_lanes::set_test_timings(LaneTimings {
        startup_delay: Duration::from_millis(100),
        window_poll: POLL_INTERVAL,
    });

    // 测试账本：非本位币（USD）账户，创建日 2022-06-15（零痕迹判据的输入显式
    // 给定，与 fx_sync 域单测同一夹具纪律）。有痕迹 → 走同步；深度由落库序列定。
    let dir = seed::make_book_dir("market-fx-lane");
    seed::create_empty_book_file(&dir);
    let codes = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.conn.lock().unwrap();
        ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "美元投资".to_owned(),
                kind: AccountType::Investment,
                currency_code: "USD".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE accounts SET created_at='2022-06-15T08:00:00Z' WHERE name='美元投资'",
            [],
        )
        .unwrap();
        dictionary_codes(&conn)
    };

    // 注入桩通道束：后台每日通道门控注入；并发一路的桩束不注入（独立通道束直构，
    // 与后台通道无任何共享互斥）。
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let lane_calls = Arc::new(AtomicUsize::new(0));
    let other_calls = Arc::new(AtomicUsize::new(0));
    let (lane_channels, other_channels) = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.conn.lock().unwrap();
        (
            stub_fx_channels(
                &conn,
                LANE_DAY_SPECS,
                lane_calls.clone(),
                Some((entered_tx, release_rx)),
            ),
            stub_fx_channels(&conn, OTHER_DAY_SPECS, other_calls.clone(), None),
        )
    };
    nsh_app::market_lanes::inject_fx_channels_for_tests(lane_channels);
    nsh_app::market_lanes::inject_price_refresh_channels_for_tests(silent_price_channels());
    nsh_app::market_lanes::inject_history_backfill_channels_for_tests(silent_price_channels());

    // 启动接线（生产装配路径同款：run_startup 成功分支单点挂载三条车道）。
    nsh_app::app::set_book_dir(dir.clone());
    assert_eq!(
        nsh_app::app::run_startup(),
        nsh_app::app::StartupView::Ready,
        "引导成功呈现就绪态"
    );
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();

    // 等每日汇率通道真实在途（门控取数点）——启动接线所拉起的调度任务跑出了
    // 首轮（删 `start_market_lanes()` 接线，本等待超时红）。
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("每日汇率同步应到达全量取数点");

    // 每日通道在途窗口内，另一路汇率同步（独立通道束 + 独立会话，手动入口并发
    // 的进程内同形，共用进程级单例门）立即被在途互斥拒绝。
    {
        let session = FacadeWriteSession::new(db.write_handle(), "daily_fx_sync_it_other");
        let mut channels = other_channels;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(sync_fx_rates_guarded(
                &FX_SYNC_GATE,
                &session,
                &mut channels,
                &mut |_| {},
            ))
            .expect_err("每日通道在途时另一路应被互斥拒绝");
        assert!(
            err.is_code("fx.sync-in-progress"),
            "每日通道在途时另一路应立即报 fx.sync-in-progress，实际 {err:?}"
        );
    }
    assert_eq!(
        other_calls.load(Ordering::SeqCst),
        0,
        "撞车即返：另一路不得触达取数"
    );
    {
        let conn = db.conn.lock().unwrap();
        assert_eq!(fx_point_count(&conn), 0, "撞车即返：另一路零落库");
    }

    // 放行后台取数：每日通道完成落库——4 个夹具周 × 字典对全落库，当期汇率
    // 改写为最新一天 2026-09-21 的交叉值（「后台通道真实落库」的可观察面）。
    // 另一路被互斥拒绝、零写入，落库全部来自每日通道。
    release_tx.send(()).expect("放行应成功");
    let expected_rows = (LANE_DAY_SPECS.len() * (codes.len() - 1)) as i64;
    let expected_rate = LANE_DAY_SPECS.last().unwrap().1 / usd_leg(&codes);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (rows, rate): (i64, Option<f64>) = {
            let conn = db.conn.lock().unwrap();
            let rows = fx_point_count(&conn);
            let rate: Option<f64> = conn
                .query_row(
                    "SELECT rate FROM exchange_rates WHERE base_code='USD' AND quote_code='CNY'",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .unwrap();
            (rows, rate)
        };
        if rows == expected_rows && rate.is_some_and(|rate| (rate - expected_rate).abs() < 1e-9) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "每日通道未在限时内完成落库（全量取数已放行：rows={rows}, rate={rate:?}）"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // 同日窗口不重跑（AC「每自然日至多一次」）：巡检多次到期后，每日通道的
    // 全量取数仍只有首轮那一次，增量腿从未被触达。
    let step = POLL_INTERVAL / 4;
    let deadline = Instant::now() + POLL_INTERVAL * 2;
    while Instant::now() < deadline {
        let calls = lane_calls.load(Ordering::SeqCst);
        if calls != 1 {
            panic!("同一自然日窗口内巡检到期不得重跑第二轮（实取 {calls} 次）");
        }
        std::thread::sleep(step);
    }
    assert_eq!(lane_calls.load(Ordering::SeqCst), 1, "同日窗口不重跑");
}
