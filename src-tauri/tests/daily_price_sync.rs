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
    BulkFetchSurfaces, DailyPriceRefreshChannelsSlot, LaneTimings, QuoteItem, SyncFetchChannels,
    start_daily_price_refresh_with,
};
use tauri_app_lib::commands::sync::{SyncChannelsSlot, sync_instrument_info};

const POLL_INTERVAL: Duration = Duration::from_millis(300);

fn gated_daily_refresh_channels(
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
    quote_calls: Arc<std::sync::atomic::AtomicUsize>,
) -> DailyPriceRefreshChannelsSlot {
    let channels = SyncFetchChannels {
        fetch_quotes: Box::new(move |_queries| {
            quote_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
    };
    DailyPriceRefreshChannelsSlot(Arc::new(tokio::sync::Mutex::new(channels)))
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
    };
    SyncChannelsSlot(Arc::new(tokio::sync::Mutex::new(channels)))
}

#[test]
fn startup_wiring_refreshes_prices_and_frontend_sync_stays_unblocked() {
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();

    let dir = tauri_app_lib::test_support::ScratchDir::new("daily-price-refresh-it");
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
    let quote_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    app.manage(gated_daily_refresh_channels(
        entered_tx,
        release_rx,
        quote_calls.clone(),
    ));
    app.manage(frontend_sync_channels());

    let price_signals: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    {
        let log = price_signals.clone();
        app.listen(events::PRICES_CHANGED, move |_| {
            *log.lock().unwrap() += 1;
        });
    }

    start_daily_price_refresh_with(
        app.handle(),
        LaneTimings {
            startup_delay: Duration::from_millis(100),
            window_poll: POLL_INTERVAL,
        },
    );

    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("每日现价刷新应到达批量报价抓取点");

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

    wait_poll_cycles(2, POLL_INTERVAL, || {
        quote_calls.load(std::sync::atomic::Ordering::SeqCst) == 1
    });
    assert_eq!(
        quote_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "同一自然日窗口内巡检到期不得重跑第二轮"
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
