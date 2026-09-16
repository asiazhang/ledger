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
use ledger_infra::error::{AppError, Result};
use ledger_infra::events;
use ledger_market_sync::{
    BulkFetchCircuit, BulkFetchSurfaces, FundNameDictionary, FundNavTable,
    INSTRUMENT_SYNC_PROGRESS, StockItem, SyncFetchChannels,
};
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
/// 现场无基金标的，净值/名称通道与两个批量取数面都不应被触达——净值分区为空时
/// 编排不试取数面，[`BulkFetchSurfaces::absent`] 只是束字段的合法占位）。
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
        bulk: BulkFetchSurfaces::absent(),
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

/// 批量面降级事实随 IPC 结果透出（ADR-0121 决策 4，issue #1376）：批量取数面
/// 失败回退逐只通道时，IPC 结果 JSON 带出 `bulk_degraded: true`；批量面正常
/// 命中路径带 `bulk_degraded: false`。断言对准**序列化 JSON**（前端经 invoke
/// 实际收到的形状）——降级字段若退回 `#[serde(skip)]`（不进 IPC 线），结构体
/// 字段断言仍绿而 IPC 面静默失真，本测试即红（接线型负向判据）。
///
/// 降级场景取「净值面命中 + 名称面失败」形态：一面失败即本次同步降级
///（ADR-0121 决策 3），而净值面命中使该基金按「无新净值」整只零逐只请求——
/// 逐只净值通道不被触达（stub unreachable 即钉住），同步仍成功返回（fail-closed
/// 不丢数据），结果带降级事实。两场景共用同一基金基线：现价缓存水位 + 历史
/// 序列各一行（批量面「无新净值」判据依赖，issue #1059 首刷判据）。
#[test]
fn bulk_degradation_fact_reaches_the_ipc_result() {
    isolate_home();
    // 提交点后置动作与交易域接缝接线（与生产启动接线同形，幂等；写入口的
    // 本位币读取依赖后者，缺装即报 transaction.base-currency-reader-unregistered
    // ——单跑本测试时无其他测试代装，必显真实缺陷）。
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = std::env::temp_dir().join(format!(
        "ledger-instrumentsync-degraded-it-{}",
        ledger_infra::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).expect("临时目录应可建");
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));

    // 静态基线：一只场外基金（fund + 6 位真实代码 → 净值分区，issue #1060 判定
    // 单点；直插 SQL 为域测试同款先例，ADR-0084 造数纪律）。
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
        // 现价缓存水位 + 历史序列：非首刷、水位同日 → 净值面命中即「无新净值」，
        // 整只零逐只请求（fund_nav 的 bulk_decision 判据）。
        guard
            .execute(
                "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,nav_date,source,created_at,updated_at,version,device_id) \
                 VALUES ('mp-1','inst-fund',33480,'CNY','2026-01-30','2026-01-30','eastmoney','2026-01-30T00:00:00Z','2026-01-30T00:00:00Z',1,'test')",
                [],
            )
            .expect("种子现价缓存应落库");
        tauri_app_lib::test_support::seed_price_history(
            &guard,
            "ph-1",
            "inst-fund",
            "2026-01-30",
            33480,
            "CNY",
        );
    }

    /// 逐只通道桩：无行情标的（报价/K 线/汇率不触达）；净值两通道不触达
    ///（净值面命中即「无新净值」零逐只请求，触达即测试场景失真）；名称通道
    /// 返回权威名称（名称面未覆盖时逐只兜底的合法应答）。
    fn per_item_channels() -> SyncFetchChannels {
        SyncFetchChannels {
            fetch_ulist: Box::new(|_| Ok(vec![])),
            fetch_kline: Box::new(|_| unreachable!("测试现场无行情标的，K 线通道不应被触达")),
            fetch_fx: Box::new(|_| unreachable!("测试现场无外币标的，汇率通道不应被触达")),
            fetch_nav: Box::new(|_| unreachable!("净值面命中即无新净值，逐只净值通道不应被触达")),
            fetch_nav_full: Box::new(|_| {
                unreachable!("净值面命中即无新净值，单请求全量净值通道不应被触达")
            }),
            fetch_fund_name: Box::new(|_| Ok("权威名称-110022".into())),
            bulk: BulkFetchSurfaces::absent(),
        }
    }

    /// 净值批量面命中桩：收录该基金且最新净值日期不晚于水位（无新净值）。
    fn nav_hit() -> Result<FundNavTable> {
        Ok(FundNavTable::from([(
            "110022".to_string(),
            ledger_market_sync::BulkNavPoint {
                date: "2026-01-30".into(),
                nav: 3.348,
            },
        )]))
    }

    // 注入通道束槽并持锁句柄：两场景经同一槽原地换装批量面（tauri manage
    // 对已存在状态不替换，二次 manage 是静默无效操作）。
    let slot = Arc::new(Mutex::new(per_item_channels()));
    app.manage(SyncChannelsSlot(slot.clone()));
    let run_sync = || {
        let handle = app.handle().clone();
        tauri::async_runtime::block_on(sync::sync_instrument_info(
            handle.state::<DbState>(),
            handle.clone(),
        ))
    };

    // 场景一（降级）：名称全量字典报错 → 一面失败即本次同步降级（净值面已命中，
    // 逐只路径不受影响）→ 结果带 `bulk_degraded: true`。
    let degraded = {
        let mut channels = per_item_channels();
        channels.bulk = BulkFetchSurfaces {
            names: Box::new(|| Err(AppError::Io("名称字典被风控拦截".into()))),
            nav: Box::new(nav_hit),
            circuit: Arc::new(Mutex::new(BulkFetchCircuit::new())),
        };
        *slot.lock().expect("通道束槽应可锁") = channels;
        run_sync().expect("降级路径同步应成功返回（fail-closed 不丢数据）")
    };
    let degraded_json = serde_json::to_string(&degraded).expect("结果应可序列化");
    assert!(
        degraded_json.contains("\"bulk_degraded\":true"),
        "降级事实应进 IPC 线，实际 {degraded_json}"
    );

    // 场景二（正常）：两个批量面全部命中 → 结果带 `bulk_degraded: false`——
    // 正常路径不带降级事实。通道束经同一槽原地换装。
    let normal = {
        let mut channels = per_item_channels();
        channels.bulk = BulkFetchSurfaces {
            names: Box::new(|| Ok(FundNameDictionary::new())),
            nav: Box::new(nav_hit),
            circuit: Arc::new(Mutex::new(BulkFetchCircuit::new())),
        };
        *slot.lock().expect("通道束槽应可锁") = channels;
        run_sync().expect("正常路径同步应成功返回")
    };
    let normal_json = serde_json::to_string(&normal).expect("结果应可序列化");
    assert!(
        normal_json.contains("\"bulk_degraded\":false"),
        "正常（批量面命中）路径不应带降级事实，实际 {normal_json}"
    );
}

/// 生产通道束构造路径的负向判据（issue #1403）：命令壳**不注入**桩通道束时
/// （生产分支）必须在阻塞线程上构造生产束并正常返回——空库在编排里早退
///（「暂无标的可同步」），生产束只被构造、不发任何网络请求，故断言确定性成立。
/// 把构造放回异步上下文（tokio worker / `block_on` 所在线程），reqwest 阻塞
/// 客户端在 debug 构建下的断言即在构造点 panic，本测试变红（修复前已在本机
/// 红过）——「删除接线即变红」的负向半边（ADR-0087 断言强度）。
#[test]
fn production_channels_construct_outside_async_context_on_empty_db() {
    isolate_home();
    // 提交点后置动作与交易域接缝接线（与上两测同形，幂等）。
    ledger_backup::install_after_commit_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = std::env::temp_dir().join(format!(
        "ledger-instrumentsync-production-it-{}",
        ledger_infra::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).expect("临时目录应可建");
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));

    // 刻意**不** manage `SyncChannelsSlot`：本例走命令壳的生产通道束分支。
    let result = tauri::async_runtime::block_on(sync::sync_instrument_info(
        app.state::<DbState>(),
        app.handle().clone(),
    ))
    .expect("空库同步应正常返回");

    assert_eq!(result.synced, 0, "空库同步不应有成功标的");
    assert_eq!(result.skipped, 0, "空库同步不应有跳过标的");
    assert_eq!(result.message, "暂无标的可同步", "空库应返回明确提示");
}
