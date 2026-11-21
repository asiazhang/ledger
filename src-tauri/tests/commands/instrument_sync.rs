use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{Listener, Manager};

use ledger_infra::db::{self, DbState};
use ledger_infra::error::{AppError, Result};
use ledger_infra::events;
use ledger_market_sync::{
    BulkFetchCircuit, BulkFetchSurfaces, FundBatch, FundNavTable, INSTRUMENT_SYNC_PROGRESS,
    QuoteItem, SyncFetchChannels,
};
use tauri_app_lib::commands::sync::SyncChannelsSlot;
use tauri_app_lib::commands::{investment, sync};

use tauri_app_lib::test_support::isolate_home;

const IN_FLIGHT_READ_TIMEOUT: Duration = Duration::from_secs(5);

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn gated_channels(
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
) -> SyncChannelsSlot {
    let channels = SyncFetchChannels {
        fetch_quotes: Box::new(move |_queries| {
            entered.send(()).expect("在途通知应可送达");
            release
                .recv_timeout(Duration::from_secs(10))
                .expect("测试应放行批量报价");
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
fn reads_return_current_data_while_sync_in_flight() {
    isolate_home();
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = tauri_app_lib::test_support::ScratchDir::new("instrumentsync-it");
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));

    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        tauri_app_lib::test_support::seed_account(
            &guard,
            "acc-1",
            "投资账户",
            "investment",
            "CNY",
            0,
        );
        tauri_app_lib::test_support::seed_instrument(
            &guard,
            "inst-1",
            "600519",
            "名称-600519",
            "CNY",
            "sh",
        );
        guard
            .execute(
                "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id) \
                 VALUES ('txn-1','buy',1000,'CNY',1000,'acc-1','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
                [],
            )
            .expect("种子交易应落库");
        guard
            .execute(
                "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
                 VALUES ('txn-1','inst-1','buy',10,100,0)",
                [],
            )
            .expect("种子批次应落库");
        guard
            .execute(
                "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
                 VALUES ('lot-1','acc-1','inst-1','txn-1',10,10,100,'CNY','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
                [],
            )
            .expect("种子持仓应落库");
    }

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    app.manage(gated_channels(entered_tx, release_rx));

    let progress_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let log = progress_log.clone();
        app.listen(INSTRUMENT_SYNC_PROGRESS, move |event| {
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

    let sync_handle = app.handle().clone();
    let sync_worker = std::thread::spawn(move || {
        tauri::async_runtime::block_on(sync::sync_instrument_info(
            sync_handle.state::<DbState>(),
            sync_handle.clone(),
        ))
    });

    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("同步应到达批量报价抓取点");
    let read_deadline = Instant::now() + IN_FLIGHT_READ_TIMEOUT;

    let holdings_rx = {
        let handle = app.handle().clone();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let json = tauri::async_runtime::block_on(async {
                let holdings = investment::list_holdings(handle.state::<DbState>())
                    .await
                    .expect("持仓读命令应成功");
                serde_json::to_string(&holdings).expect("持仓应可序列化")
            });
            let _ = tx.send(json);
        });
        rx
    };
    let holdings = holdings_rx
        .recv_timeout(remaining(read_deadline))
        .unwrap_or_else(|_| panic!("持仓读命令应在时限内返回（同步在途不挡读）"));
    assert!(
        holdings.contains("\"instrument_id\":\"inst-1\"") && holdings.contains("\"quantity\":10.0"),
        "在途窗口内持仓读应返回当前已提交数据，实际 {holdings}"
    );

    let instruments_rx = {
        let handle = app.handle().clone();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let json = tauri::async_runtime::block_on(async {
                let instruments = investment::list_instruments(handle.state::<DbState>(), None)
                    .await
                    .expect("标的读命令应成功");
                serde_json::to_string(&instruments).expect("标的应可序列化")
            });
            let _ = tx.send(json);
        });
        rx
    };
    let instruments = instruments_rx
        .recv_timeout(remaining(read_deadline))
        .unwrap_or_else(|_| panic!("标的读命令应在时限内返回（同步在途不挡读）"));
    assert!(
        instruments.contains("600519"),
        "在途窗口内标的读应返回当前已提交数据，实际 {instruments}"
    );

    assert!(
        progress_log
            .lock()
            .unwrap()
            .iter()
            .any(|payload| payload.contains("\"done\":0") && payload.contains("\"total\":1")),
        "在途窗口内应可见确定进度事件，实际 {:?}",
        progress_log.lock().unwrap()
    );

    release_tx.send(()).expect("放行应成功");
    let result = sync_worker
        .join()
        .expect("同步命令应跑完")
        .expect("同步应成功");
    assert_eq!(result.synced, 1, "报价有效应计同步");
    let prices =
        tauri::async_runtime::block_on(investment::list_market_prices(app.state::<DbState>()))
            .expect("现价读应成功");
    assert!(
        serde_json::to_string(&prices)
            .expect("现价应可序列化")
            .contains("130280"),
        "同步完成后新价应可读，实际 {prices:?}"
    );
    assert_eq!(
        *price_signals.lock().unwrap(),
        1,
        "成功且实际写入应发恰好一次价格失效信号（映射单点判定）"
    );
}

#[test]
fn in_memory_reads_return_while_sync_in_flight() {
    isolate_home();
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let app = tauri::test::mock_app();
    let shared = std::sync::Arc::new(std::sync::Mutex::new(tauri_app_lib::test_support::open()));
    app.manage(db::DbState {
        conn: std::sync::Arc::clone(&shared),
        read_conn: shared,
    });

    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
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
    app.manage(gated_channels(entered_tx, release_rx));

    let sync_handle = app.handle().clone();
    let sync_worker = std::thread::spawn(move || {
        tauri::async_runtime::block_on(sync::sync_instrument_info(
            sync_handle.state::<DbState>(),
            sync_handle.clone(),
        ))
    });

    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("同步应到达批量报价抓取点");
    let read_deadline = Instant::now() + IN_FLIGHT_READ_TIMEOUT;

    let holdings_rx = {
        let handle = app.handle().clone();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let json = tauri::async_runtime::block_on(async {
                let holdings = investment::list_holdings(handle.state::<DbState>())
                    .await
                    .expect("持仓读命令应成功");
                serde_json::to_string(&holdings).expect("持仓应可序列化")
            });
            let _ = tx.send(json);
        });
        rx
    };
    let holdings = holdings_rx
        .recv_timeout(remaining(read_deadline))
        .unwrap_or_else(|_| panic!("持仓读命令应在时限内返回（同步在途不挡读）"));
    assert!(
        serde_json::from_str::<serde_json::Value>(&holdings).is_ok(),
        "在途窗口内持仓读应返回合法应答，实际 {holdings}"
    );

    let instruments_rx = {
        let handle = app.handle().clone();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let json = tauri::async_runtime::block_on(async {
                let instruments = investment::list_instruments(handle.state::<DbState>(), None)
                    .await
                    .expect("标的读命令应成功");
                serde_json::to_string(&instruments).expect("标的应可序列化")
            });
            let _ = tx.send(json);
        });
        rx
    };
    let instruments = instruments_rx
        .recv_timeout(remaining(read_deadline))
        .unwrap_or_else(|_| panic!("标的读命令应在时限内返回（同步在途不挡读）"));
    assert!(
        instruments.contains("600519"),
        "在途窗口内标的读应返回当前已提交数据，实际 {instruments}"
    );

    release_tx.send(()).expect("放行应成功");
    let result = sync_worker
        .join()
        .expect("同步命令应跑完")
        .expect("同步应成功");
    assert_eq!(result.synced, 1, "报价有效应计同步");
}

#[test]
fn bulk_degradation_fact_reaches_the_ipc_result() {
    isolate_home();
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = tauri_app_lib::test_support::ScratchDir::new("instrumentsync-degraded-it");
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));

    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        tauri_app_lib::test_support::seed_instrument(
            &guard,
            "inst-fund",
            "110022",
            "易方达消费行业",
            "CNY",
            "unknown",
        );
        guard
            .execute(
                "UPDATE instruments SET instrument_type='fund' WHERE id='inst-fund'",
                [],
            )
            .expect("种子基金类型应落库");
        tauri_app_lib::test_support::seed_fund_market_price(
            &guard,
            "inst-fund",
            33_480,
            "CNY",
            "2026-01-30",
            Some("eastmoney"),
        );
        tauri_app_lib::test_support::seed_price_history(
            &guard,
            "ph-1",
            "inst-fund",
            "2026-01-30",
            33480,
            "CNY",
        );
    }

    fn per_item_channels() -> SyncFetchChannels {
        SyncFetchChannels {
            fetch_quotes: Box::new(|_| Box::pin(async { Ok(vec![]) })),
            fetch_kline: Box::new(|_| {
                Box::pin(async {
                    unreachable!("测试现场无行情标的，K 线通道不应被触达")
                })
            }),
            fetch_nav_history: Box::new(|_| {
                Box::pin(async {
                    unreachable!("净值面命中即无新净值，逐只净值通道不应被触达")
                })
            }),
            fetch_fund_name: Box::new(|_| Box::pin(async { Ok("权威名称-110022".into()) })),
            confirm_money_fund_form: Box::new(|_| Box::pin(async { Ok(false) })),
            bulk: BulkFetchSurfaces::absent(),
        }
    }

    fn nav_hit() -> Pin<Box<dyn Future<Output = Result<FundBatch>> + Send>> {
        Box::pin(async {
            Ok(FundBatch {
                names: [("110022".to_string(), "易方达消费行业A".to_string())]
                    .into_iter()
                    .collect(),
                nav: FundNavTable::from([(
                    "110022".to_string(),
                    ledger_market_sync::BulkNavPoint {
                        date: "2026-01-30".into(),
                        nav: 3.348,
                    },
                )]),
            })
        })
    }

    let slot = Arc::new(tokio::sync::Mutex::new(per_item_channels()));
    app.manage(SyncChannelsSlot(slot.clone()));
    let run_sync = || {
        let handle = app.handle().clone();
        tauri::async_runtime::block_on(sync::sync_instrument_info(
            handle.state::<DbState>(),
            handle.clone(),
        ))
    };

    let degraded = {
        let mut channels = per_item_channels();
        channels.fetch_nav_history = Box::new(|_| Box::pin(async { Ok(vec![]) }));
        channels.bulk = BulkFetchSurfaces {
            funds: Box::new(|_: &[String]| {
                Box::pin(async { Err(AppError::Io("场外基金批量面被风控拦截".into())) })
            }),
            circuit: Arc::new(Mutex::new(BulkFetchCircuit::new())),
        };
        *slot.blocking_lock() = channels;
        run_sync().expect("降级路径同步应成功返回（fail-closed 不丢数据）")
    };
    let degraded_json = serde_json::to_string(&degraded).expect("结果应可序列化");
    assert!(
        degraded_json.contains("\"bulk_degraded\":true"),
        "降级事实应进 IPC 线，实际 {degraded_json}"
    );

    let normal = {
        let mut channels = per_item_channels();
        channels.bulk = BulkFetchSurfaces {
            funds: Box::new(|_: &[String]| nav_hit()),
            circuit: Arc::new(Mutex::new(BulkFetchCircuit::new())),
        };
        *slot.blocking_lock() = channels;
        run_sync().expect("正常路径同步应成功返回")
    };
    let normal_json = serde_json::to_string(&normal).expect("结果应可序列化");
    assert!(
        normal_json.contains("\"bulk_degraded\":false"),
        "正常（批量面命中）路径不应带降级事实，实际 {normal_json}"
    );
}

#[test]
fn production_channels_branch_runs_empty_db_to_early_exit() {
    isolate_home();
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = tauri_app_lib::test_support::ScratchDir::new("instrumentsync-production-it");
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));

    let result = tauri::async_runtime::block_on(sync::sync_instrument_info(
        app.state::<DbState>(),
        app.handle().clone(),
    ))
    .expect("空库同步应正常返回");

    assert_eq!(result.synced, 0, "空库同步不应有成功标的");
    assert_eq!(result.skipped, 0, "空库同步不应有跳过标的");
    assert_eq!(result.message, "暂无标的可同步", "空库应返回明确提示");
}
