//! 后台每日汇率增量同步接线测试（ADR-0019 修订记录 / issue #1546）。
//!
//! **独立测试二进制**（不经 tests/commands 目标）：调度任务的单次拉起守卫是
//! 进程级单例，接线行为断言须在独立进程现场注入短时机（先例：
//! `tests/daily_price_sync.rs`，issue #1377 同形）。
//!
//! 断言全部对准用户可观察结果（CONTEXT-testing〈断言强度〉）：
//! - 启动接线真实拉起每日汇率通道：账本出现非本位币痕迹后，一轮同步把汇率
//!   历史与当期汇率自己写进库（fx_rate_history / exchange_rates 落行）。本测试
//!   直接驱动域启动入口；「删除 lib.rs 启动接线即变红」的负向判据由
//!   `scripts/check-background-services.ts` 源码扫描守门承担（#959 / #961 先例）；
//! - 每日通道在途（门控桩阻塞在取数点）时，另一路汇率同步（独立通道束 +
//!   独立会话，另一台设备 / 手动入口并发的进程内同形）照常完成、不互斥——
//!   各自幂等落库，不引入「只允许一个同步方」的开关（issue #1546 AC4）；
//! - 两路覆盖区间重叠三周：重叠段不重复落行、每日通道多出的一周与当期汇率
//!   改写照常落库（重复落库幂等，issue #1546 AC2）；
//! - 同一自然日窗口不重跑（「每自然日至多一次」，issue #1546 AC2）。
//!
//! 失败面的行为断言归域单测：三态错误分类住 market-sync `tests/fx_sync.rs`、
//! 单事务原子（失败不留部分写入）与人工行保护住 `tests/fx_persist.rs`。本车道
//! 的失败处置是「记 warn 日志可见 + 静默等下一自然日窗口 + 已有汇率不受影响」
//!——取数发生在落库之前，取数失败即零写入，不另设进程内断言面。

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
use rusqlite::Connection;
use tauri::Manager;

use ledger_infra::db::{self, DbState};
use ledger_market_sync::{
    DailyFxSyncChannelsSlot, EcbDayRates, FacadeWriteSession, FxSyncChannels, FxSyncReport,
    LaneTimings, start_daily_fx_sync_with, sync_fx_rates,
};

/// 每日汇率通道巡检周期（与下方注入的 `LaneTimings::window_poll` 同源）：
/// 同日窗口的观察窗按它取整周期推导，不另写时长字面量。
const POLL_INTERVAL: Duration = Duration::from_millis(300);

/// 字典币种（种子库全量，本位币 CNY 在内）——通道桩按字典全量腿构造日快照。
fn dictionary_codes(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT code FROM currencies ORDER BY code")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

/// 一天快照：腿覆盖字典全部币种（编排按字典全量对本位币推导，缺腿的币种对无点）。
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

/// 后台桩的四天快照（窗口起点周 / 痕迹日当周 / 近期两周）：覆盖区间与并发桩
///（三天快照）重叠三周、多出一周——放行后多出的那一周与当期汇率的改写是
/// 「后台通道真实落库」的可观察面（数据完全相同时落库不可区分）。
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

/// 全量腿的形态：门控（每日通道——取数点先通知「已在途」再等放行）或直通
///（并发一路）。
enum FullLeg {
    Gated {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    },
    Immediate,
}

/// 桩通道束构造单点（另一台设备 / 手动入口的进程内同形：与后台通道无任何
/// 共享互斥，两路并发各写各的）：快照腿覆盖字典全量币种、全量腿按 [`FullLeg`]
/// 门控或直通并各自计数（供「同日窗口不重跑」与「并发一路只取数一次」断言
/// 消费）、增量腿在深度未达的首轮不该被触达（unreachable 兜底）。
fn stub_fx_channels(
    conn: &Connection,
    specs: &[(&str, f64)],
    full_calls: Arc<AtomicUsize>,
    leg: FullLeg,
) -> FxSyncChannels {
    let days: Vec<EcbDayRates> = specs.iter().map(|(d, cny)| day(conn, d, *cny)).collect();
    FxSyncChannels {
        fetch_full: Box::new(move || {
            full_calls.fetch_add(1, Ordering::SeqCst);
            if let FullLeg::Gated { entered, release } = &leg {
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

/// 汇率历史总行数（落库面的可观察合计）。
fn fx_point_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM fx_rate_history", [], |r| r.get(0))
        .unwrap()
}

/// 接线全流程：启动接线跑出每日汇率同步（汇率历史 + 当期汇率落行），且每日
/// 通道在途时另一路同步照常完成、不互斥、各自幂等落库；同一自然日窗口不重跑。
/// 删掉 `start_background_services` 的 `start_daily_fx_sync` 接线（或让每日通道
/// 与另一路互斥）本测试红。
#[test]
fn startup_wiring_syncs_fx_rates_and_concurrent_sync_stays_unblocked() {
    // 交易域接缝接线（本位币读取钩子）：窗口判据读默认币种经该钩子，与生产
    // 启动接线同形，幂等（先例：tests/daily_price_sync.rs）。
    tauri_app_lib::transaction_wiring::install_all();

    // 设备现场（daily_price_sync IT 同款）：mock 应用 + 独立临时目录文件库 +
    // 两扇门（调度任务做空转判定）。
    let dir = std::env::temp_dir().join(format!(
        "ledger-daily-fx-sync-it-{}",
        ledger_infra::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(ledger_infra::db::encryption::EncryptionGate::new(false));
    app.manage(ledger_infra::db::boot::BootFailureGate::new());

    // 静态基线：非本位币（USD）账户，创建日 2022-06-15（窗口判据输入显式给定，
    // 与 fx_sync 域单测同一夹具纪律）。窗口起点 = 2022-06-13 所在周周一再前推
    // 一周 = 2022-06-06。
    let conn = app.state::<DbState>().conn.clone();
    let codes = {
        let guard = conn.lock().unwrap();
        tauri_app_lib::test_support::seed_account(
            &guard,
            "acc-usd",
            "美元投资",
            "investment",
            "USD",
            0,
        );
        guard
            .execute(
                "UPDATE accounts SET created_at='2022-06-15T08:00:00Z' WHERE id='acc-usd'",
                [],
            )
            .unwrap();
        dictionary_codes(&guard)
    };

    // 注入桩通道束：后台每日通道经槽接缝门控注入；并发一路的桩束不管理
    //（独立通道束直构）。
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let lane_calls = Arc::new(AtomicUsize::new(0));
    let other_calls = Arc::new(AtomicUsize::new(0));
    let (lane_channels, other_channels) = {
        let guard = conn.lock().unwrap();
        (
            stub_fx_channels(
                &guard,
                LANE_DAY_SPECS,
                lane_calls.clone(),
                FullLeg::Gated {
                    entered: entered_tx,
                    release: release_rx,
                },
            ),
            stub_fx_channels(
                &guard,
                OTHER_DAY_SPECS,
                other_calls.clone(),
                FullLeg::Immediate,
            ),
        )
    };
    app.manage(DailyFxSyncChannelsSlot(Arc::new(tokio::sync::Mutex::new(
        lane_channels,
    ))));

    // 启动接线（生产唯一编排点的同款调用）：注入短时机（生产 30 秒延迟）。
    start_daily_fx_sync_with(
        app.handle(),
        LaneTimings {
            startup_delay: Duration::from_millis(100),
            // 巡检周期取短：轮询在窗口内多次到期，同日不得重跑（下方断言）。
            window_poll: POLL_INTERVAL,
        },
    );

    // 等每日汇率通道真实在途（门控取数点）——启动接线所拉起的调度任务跑出
    // 了首轮（删接线 / 删域入口，本等待超时红）。
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("每日汇率同步应到达全量取数点");

    // 每日通道在途窗口内，另一路汇率同步（独立通道束 + 独立会话）照常完成、
    // 不被互斥拒绝：窗口判据、取数、落库全链路走完——多设备 / 手动入口并发
    // 的进程内同形（issue #1546 AC4：各自幂等落库，无「只允许一个同步方」开关）。
    let other_report: FxSyncReport = {
        let session = FacadeWriteSession::new(
            app.state::<DbState>().write_handle(),
            "daily_fx_sync_it_other",
        );
        let mut channels = other_channels;
        tauri::async_runtime::block_on(sync_fx_rates(&session, &mut channels))
            .expect("每日通道在途时另一路同步不应被拒绝")
    };
    assert!(
        other_report.full_backfilled,
        "并发一路按判据走全量回填（此刻库内尚无任何汇率行）"
    );

    // 并发一路的落库可观察：窗口起点 2022-06-06 起 3 个周 × 字典 10 对全落库；
    // 当期汇率表 USD/CNY = 最新一天的交叉值（CNY 腿 ÷ USD 腿），来源 ECB。
    {
        let guard = conn.lock().unwrap();
        assert_eq!(fx_point_count(&guard), 30, "10 对 × 3 个窗口内周全落库");
        let earliest: String = guard
            .query_row(
                "SELECT MIN(week_start) FROM fx_rate_history WHERE base_code='USD'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(earliest, "2022-06-06", "窗口起点周即最早周");
        let (rate, source): (f64, String) = guard
            .query_row(
                "SELECT rate, source FROM exchange_rates WHERE base_code='USD' AND quote_code='CNY'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(source, "ecb");
        let expected = 7.4 / usd_leg(&codes);
        assert!(
            (rate - expected).abs() < 1e-9,
            "当期值 = 最新一天交叉值（{rate} vs {expected}）"
        );
    }

    // 放行后台取数：每日通道完成落库——多出的那一周（第 4 周）与当期汇率的
    // 改写（最新一天 2026-09-21 的交叉值）是「后台通道真实落库」的可观察面；
    // 重叠的三周不产生重复行（两路各写一遍，落库行是 4 周 × 10 对 = 40 而非
    // 30 + 30，整周覆盖幂等，issue #1546 AC2）。
    release_tx.send(()).expect("放行应成功");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (rows, rate): (i64, f64) = {
            let guard = conn.lock().unwrap();
            let rows = fx_point_count(&guard);
            let rate: f64 = guard
                .query_row(
                    "SELECT rate FROM exchange_rates WHERE base_code='USD' AND quote_code='CNY'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            (rows, rate)
        };
        if rows == 40 && (rate - 7.5 / usd_leg(&codes)).abs() < 1e-9 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "每日通道未在限时内完成落库（全量取数已放行：rows={rows}, rate={rate}）"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    {
        let guard = conn.lock().unwrap();
        assert_eq!(
            fx_point_count(&guard),
            40,
            "重叠三周不重复落行（整周覆盖幂等），第 4 周新增（10 对 × 4 周）"
        );
    }

    // 同日窗口不重跑（AC「每自然日至多一次」）：巡检多次到期后，每日通道的
    // 全量取数仍只有首轮那一次，增量腿从未被触达。
    //
    // 规则本身（同日不开、跨日开）的权威在 market-sync 域单测
    // `daily_window_opens_only_once_per_beijing_calendar_day`；本断言守的是
    // **接线**——调度循环确实把判定接上了「标记已跑 + 跑一轮」的副作用。观察窗
    // 取「两次巡检周期」而非固定睡眠：等待长度由此刻生效的注入周期决定（先例：
    // tests/daily_price_sync.rs）。
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
    assert_eq!(other_calls.load(Ordering::SeqCst), 1, "并发一路只取数一次");
}
