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

const POLL_INTERVAL: Duration = Duration::from_millis(300);

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

#[test]
fn startup_wiring_backfills_history_and_frontend_sync_stays_unblocked() {
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();

    let dir = tauri_app_lib::test_support::ScratchDir::new("history-backfill-it");
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(ledger_infra::db::encryption::EncryptionGate::new(false));
    app.manage(ledger_infra::db::boot::BootFailureGate::new());

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

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let kline_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    app.manage(gated_backfill_channels(
        entered_tx,
        release_rx,
        kline_calls.clone(),
    ));
    app.manage(frontend_sync_channels());

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

    start_history_backfill_with(
        app.handle(),
        LaneTimings {
            startup_delay: Duration::from_millis(100),
            window_poll: POLL_INTERVAL,
        },
    );

    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("后台补全应到达周 K 抓取点");

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

    {
        let progress = progress_log.lock().unwrap();
        assert!(!progress.is_empty(), "后台补全应发出静默标的级计数进度事件");
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
