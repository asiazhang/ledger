//! 标的信息同步在途可读性集成测试（issue #1276，父 spec #1274 路线 A 验收）。
//!
//! 权威断言层（ADR-0087）：「同步真实在途时读命令能在时限内返回当前已提交
//! 数据」的失败面在壳层锁跨度，修复落在命令壳的分段写入口，故权威测试在此。
//! 复现手段是命令壳的同步网络通道注入接缝（`SyncChannelsSlot`，issue #1276）：
//! 注入门控桩通道束，同步在批量报价抓取点真实在途（阻塞在会话与锁之外），
//! 此时直调读命令（持仓、标的）断言及时返回；把命令壳改回整段持锁，读命令
//! 会在锁上等满整次同步——限时断言即红（负向判据，删除接线即变红）。
//!
//! 断言全部对准用户可观察结果（读命令在时限内返回、返回内容为当前已提交
//! 数据、进度事件照发、成功后价格与失效信号可见），不对准线程、锁对象或
//! 函数调用形状（CONTEXT-testing〈断言强度〉）。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{Listener, Manager};

use ledger_infra::db::{self, DbState};
use ledger_infra::events;
use ledger_market_sync::{INSTRUMENT_SYNC_PROGRESS, StockItem, SyncFetchChannels};
use tauri_app_lib::commands::sync::SyncChannelsSlot;
use tauri_app_lib::commands::{investment, sync};

use crate::isolation::isolate_home;

/// 同步在途窗口的读命令时限：分段取锁下读命令毫秒级返回；整段持锁下读命令
/// 至少要等同步收尾（门不放行即永不返回），限时断言必然超时变红。
const IN_FLIGHT_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// 距时限的剩余等待（recv_timeout 用；已过点给零时长的胜出判据）。
fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

/// 门控批量报价桩的通道束：首次批量报价（同步的会话外网络等待点）先通知
/// 「已在途」，再等测试放行并返回一条有效报价。其余通道全部空应答（测试
/// 现场无基金标的，净值/名称通道不应被触达）。
fn gated_channels(
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
) -> SyncChannelsSlot {
    let channels = SyncFetchChannels {
        fetch_ulist: Box::new(move |_secids| {
            entered.send(()).expect("在途通知应可送达");
            release
                .recv_timeout(Duration::from_secs(10))
                .expect("测试应放行批量报价");
            Ok(vec![StockItem {
                code: "600519".into(),
                name: "贵州茅台".into(),
                price: Some(1302.80),
                precision: None,
            }])
        }),
        fetch_kline: Box::new(|_| Ok(vec![])),
        fetch_fx: Box::new(|_| Ok(vec![])),
        fetch_nav: Box::new(|_| unreachable!("测试现场无基金标的，净值通道不应被触达")),
        fetch_nav_full: Box::new(|_| unreachable!("测试现场无基金标的，全量净值通道不应被触达")),
        fetch_fund_name: Box::new(|_| unreachable!("测试现场无基金标的，名称通道不应被触达")),
    };
    SyncChannelsSlot(Arc::new(Mutex::new(channels)))
}

/// 同步在途时读命令照常出数（issue #1276 核心验收 + 负向判据）：
/// 同步阻塞在批量报价抓取点（会话之外）期间，持仓与标的读命令在时限内返回
/// 且内容为当前已提交数据；确定进度照旧可见；放行后同步完成、新价可读、
/// 价格失效信号发出。把命令壳改回整段持锁，读命令会在锁上等满整次同步，
/// 限时断言超时变红。
#[test]
fn reads_return_current_data_while_sync_in_flight() {
    isolate_home();
    // 提交点后置动作接线（置脏断言依赖；与生产启动接线同形，幂等）。
    ledger_backup::install_after_commit_hook();
    // 交易域接缝接线（fresh_app 同款）：编排的本位币读取钩子随此装入（幂等）。
    tauri_app_lib::transaction_wiring::install_all();
    let dir = std::env::temp_dir().join(format!(
        "ledger-instrumentsync-it-{}",
        ledger_infra::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).expect("临时目录应可建");
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));

    // 静态基线：投资账户 + 沪市股票标的 + 一笔买入批次（带通道标的无离线
    // 公开创建路径，直插 SQL 为域测试同款先例；ADR-0084 造数纪律）。
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

    // 注入门控通道束并登记同步通道槽（命令换装的接缝）。
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    app.manage(gated_channels(entered_tx, release_rx));

    // 订阅两类用户可观察事件：确定进度与价格失效信号（mock runtime 下发射
    // 内联送达；超时上界哲学与 signal_delivery 同款）。
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

    // 同步在途：独立线程 block_on 驱动命令（写入口 → run_db 阻塞线程池与
    // 生产同链），批量报价门未放行前同步持续在途。
    let sync_handle = app.handle().clone();
    let sync_worker = std::thread::spawn(move || {
        tauri::async_runtime::block_on(sync::sync_instrument_info(
            sync_handle.state::<DbState>(),
            sync_handle.clone(),
        ))
    });

    // 等同步真实在途（批量报价抓取点），此后读命令限时直调：读者线程自带
    // 句柄独立运行，主线程只在时限内等结果——整段持锁回归下读者在锁上等满
    // 同步，此处必超时失败（负向判据）。
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

    // 在途的确定进度照旧可见：收集完成即发 {done:0,total:1}（门放行前抵达）。
    assert!(
        progress_log
            .lock()
            .unwrap()
            .iter()
            .any(|payload| payload.contains("\"done\":0") && payload.contains("\"total\":1")),
        "在途窗口内应可见确定进度事件，实际 {:?}",
        progress_log.lock().unwrap()
    );

    // 放行：同步收尾（新价落库 + 名称随行刷新 → 实际写入 → 价格失效信号）。
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
