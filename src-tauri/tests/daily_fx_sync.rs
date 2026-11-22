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
use rusqlite::{Connection, OptionalExtension};
use tauri::Manager;

use ledger_infra::db::{self, DbState};
use ledger_market_sync::{
    DailyFxSyncChannelsSlot, EcbDayRates, FX_SYNC_GATE, FacadeWriteSession, FxSyncChannels,
    LaneTimings, start_daily_fx_sync_with, sync_fx_rates_guarded,
};

const POLL_INTERVAL: Duration = Duration::from_millis(300);

fn dictionary_codes(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT code FROM currencies ORDER BY code")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

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

fn usd_leg(codes: &[String]) -> f64 {
    let idx = codes.iter().position(|c| c == "USD").unwrap();
    1.0 + (idx as f64) / 10.0
}

const LANE_DAY_SPECS: &[(&str, f64)] = &[
    ("2022-06-06", 7.2),
    ("2022-06-15", 7.3),
    ("2026-09-14", 7.4),
    ("2026-09-21", 7.5),
];

const OTHER_DAY_SPECS: &[(&str, f64)] = &[
    ("2022-06-06", 7.2),
    ("2022-06-15", 7.3),
    ("2026-09-14", 7.4),
];

enum FullLeg {
    Gated {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    },
    Immediate,
}

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

fn fx_point_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM fx_rate_history", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn startup_wiring_syncs_fx_rates_and_concurrent_sync_is_rejected() {
    tauri_app_lib::transaction_wiring::install_all();

    let dir = tauri_app_lib::test_support::ScratchDir::new("daily-fx-sync-it");
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(ledger_infra::db::encryption::EncryptionGate::new(false));
    app.manage(ledger_infra::db::boot::BootFailureGate::new());

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

    start_daily_fx_sync_with(
        app.handle(),
        LaneTimings {
            startup_delay: Duration::from_millis(100),
            window_poll: POLL_INTERVAL,
        },
    );

    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("每日汇率同步应到达全量取数点");

    {
        let session = FacadeWriteSession::new(
            app.state::<DbState>().write_handle(),
            "daily_fx_sync_it_other",
        );
        let mut channels = other_channels;
        let err = tauri::async_runtime::block_on(sync_fx_rates_guarded(
            &FX_SYNC_GATE,
            &session,
            &mut channels,
            &mut |_| {},
        ))
        .unwrap_err();
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
        let guard = conn.lock().unwrap();
        assert_eq!(fx_point_count(&guard), 0, "撞车即返：另一路零落库");
    }

    release_tx.send(()).expect("放行应成功");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (rows, rate): (i64, Option<f64>) = {
            let guard = conn.lock().unwrap();
            let rows = fx_point_count(&guard);
            let rate: Option<f64> = guard
                .query_row(
                    "SELECT rate FROM exchange_rates WHERE base_code='USD' AND quote_code='CNY'",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .unwrap();
            (rows, rate)
        };
        if rows == 40 && rate.is_some_and(|rate| (rate - 7.5 / usd_leg(&codes)).abs() < 1e-9) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "每日通道未在限时内完成落库（全量取数已放行：rows={rows}, rate={rate:?}）"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    {
        let guard = conn.lock().unwrap();
        assert_eq!(
            fx_point_count(&guard),
            40,
            "每日通道 4 个夹具周 × 10 对全落库（另一路被互斥拒绝、零写入）"
        );
    }

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
    assert_eq!(
        other_calls.load(Ordering::SeqCst),
        0,
        "另一路被互斥拒绝，全程零取数"
    );
}
