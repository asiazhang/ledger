//! 多端同步命令面集成测试（issue #862 / #863 / ADR-0091）。
//!
//! 直调命令函数（`#[tokio::test]`），覆盖壳行为：参数解包、错误码、双端经
//! 真实 WebDAV 桩手动同步后数据一致（验收判据「手动触发路径」）、挂起通知
//! 命令面可见（#863）。通道布局、轮次协议、幂等重放、挂起语义与触发编排归域
//! 单测（`sync_engine::tests` / `sync_engine::trigger::tests`，ADR-0087），
//! 此处只钉「命令壳 → 通道在位性 → 轮次 → 状态回显」的整链行为。
//!
//! 现场隔离：每个「设备」是独立的 mock 应用 + 独立临时目录的文件库 + 引导
//! 登记态（与 `books.rs` 同型）；`$HOME` 重定向收在本测试目标唯一的
//! [`crate::isolation`]（进程内一次，mock runtime 的 `app_data_dir` 回退解析
//! 隔离）。测试间以各自独立的 WebDAV 桩（真实 HTTP 服务，MKCOL/GET/PUT）对接，
//! 互不共享通道。

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

use std::path::PathBuf;

use tauri::Manager;
use tauri_app_lib::commands::accounts;
use tauri_app_lib::commands::sync_channel::{
    SyncChannelConfigInput, get_parked_ops, get_sync_channel_config, get_sync_status,
    set_sync_channel_config, sync_now,
};
use tauri_app_lib::commands::{boot::BootCell, transactions};
use tauri_app_lib::db::data_location;
use tauri_app_lib::db::encryption::{enable_encryption_for_file, probe_file_kind};
use tauri_app_lib::db::{self, DbState};
use tauri_app_lib::error::AppError;
use tauri_app_lib::sync_engine::EnvelopeMode;
use tauri_app_lib::test_support::{read_scalar_i64, spawn_webdav_stub};
use tauri_app_lib::transaction::{TransactionInput, TransactionKind};

use crate::isolation::isolate_home;

/// 引导登记态在位的 mock 应用 + 独立临时目录（不含库连接；连接由调用方按
/// 明文/密文形态自行挂载，tauri manage 同型仅首次生效）。
pub(crate) fn fresh_app(tag: &str) -> (tauri::App<tauri::test::MockRuntime>, PathBuf) {
    // 写路径副作用接缝接线（issue #1090）：本套件经产品建缝拿文件库连接（不入
    // 测试工厂，ADR-0084 决策 3），建库单点的注册不覆盖本处——落库前显式注册
    // 余额刷新实现（幂等，进程级，与 BDD world 同款纪律）。
    tauri_app_lib::accounts::balance::install_balance_refresh_hook();
    // 交易域接缝接线（issue #1092 / #1180）：与测试工厂同形——六向实现经组合入口
    // 一次装入（幂等，进程级）。
    tauri_app_lib::transaction_wiring::install_all();
    let dir = std::env::temp_dir().join(format!(
        "ledger-syncchannel-it-{tag}-{}",
        tauri_app_lib::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let app = tauri::test::mock_app();
    let boot = data_location::boot(&dir);
    app.manage(BootCell::new(boot));
    (app, dir)
}

/// 一台「设备」：mock 应用 + 独立临时目录文件库 + 真实引导登记态（BootCell）。
pub(crate) fn device_app(tag: &str) -> (tauri::AppHandle<tauri::test::MockRuntime>, PathBuf) {
    let (app, dir) = fresh_app(tag);
    app.manage(db::open_db_in(&dir).unwrap());
    (app.handle().clone(), dir)
}

/// 断言码化错误命中的稳定码。
fn assert_code(err: AppError, code: &str) {
    assert!(err.is_code(code), "期望 {code}，实际 {err:?}");
}

/// 支出交易输入构造器（行为前置经壳层公开命令，op 产出随之发生）。
pub(crate) fn expense_input(account_id: &str, amount_cents: i64, note: &str) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: Some(note.into()),
        date: "2026-01-10".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

pub(crate) const STUB_USER: &str = "alice";
pub(crate) const STUB_PASS: &str = "app-pass";

/// 两端配置同一 WebDAV 桩与同一同步空间（跨端共识的世界身份）。
pub(crate) async fn configure_channel(
    app: &tauri::AppHandle<tauri::test::MockRuntime>,
    base_url: &str,
) {
    let input = SyncChannelConfigInput {
        base_url: base_url.into(),
        username: STUB_USER.into(),
        password: STUB_PASS.into(),
        space_id: Some("family".into()),
    };
    set_sync_channel_config(app.clone(), input)
        .await
        .unwrap_or_else(|e| panic!("通道配置应保存成功: {e:?}"));
}

#[tokio::test]
async fn sync_status_defaults_and_channel_config_roundtrip() {
    isolate_home();
    let (app, _dir) = device_app("status");

    // 未配置现场：状态各字段取默认，设备标识在位（首用生成）。
    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert!(!status.device_id.is_empty(), "设备标识应非空");
    assert!(!status.channel_configured, "新端通道未配置");
    assert_eq!(status.last_sync_at, None, "从未同步");
    assert_eq!(status.parked_count, 0, "新端无挂起");
    assert!(!status.library_encrypted, "明文库");

    // 配置往返：保存 → 读回一致 + 状态翻转。
    configure_channel(&app, "http://127.0.0.1:1/dav/").await;
    let config = get_sync_channel_config(app.clone())
        .await
        .expect("配置应可读");
    assert!(config.configured);
    assert_eq!(config.base_url, "http://127.0.0.1:1/dav/");
    assert_eq!(config.username, STUB_USER);
    assert_eq!(config.password, STUB_PASS);

    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert!(status.channel_configured, "保存后通道在位");

    // 非法通道（空地址）：码化错误、不落库（读回仍是原值）。
    let bad = SyncChannelConfigInput {
        base_url: "   ".into(),
        username: STUB_USER.into(),
        password: STUB_PASS.into(),
        space_id: None,
    };
    let err = set_sync_channel_config(app.clone(), bad)
        .await
        .expect_err("空地址应被拒");
    assert_code(err, "sync-channel.base-url-missing");
    let config = get_sync_channel_config(app.clone())
        .await
        .expect("配置应可读");
    assert_eq!(
        config.base_url, "http://127.0.0.1:1/dav/",
        "坏值未覆盖原配置"
    );
}

#[tokio::test]
async fn sync_now_without_channel_is_rejected_with_coded_error() {
    isolate_home();
    let (app, _dir) = device_app("unconfigured");

    let err = sync_now(app.clone(), None).await.expect_err("未配置应被拒");
    assert_code(err, "sync-channel.not-configured");
}

/// 验收判据（issue #862）：两台桌面设备经真实 WebDAV 桩手动触发同步后数据
/// 一致。A 记账 → A 同步 → B 同步（收到 A 的账）→ B 记账 → B 同步 → A 同步
///（收到 B 的账）；业务行（账户 + 两笔交易金额）在两端相等。
#[tokio::test]
async fn dual_device_manual_sync_converges_over_real_webdav_stub() {
    isolate_home();
    let stub = spawn_webdav_stub(Some((STUB_USER, STUB_PASS)));
    let (app_a, _dir_a) = device_app("dual-a");
    let (app_b, _dir_b) = device_app("dual-b");
    configure_channel(&app_a, &stub.base_url).await;
    configure_channel(&app_b, &stub.base_url).await;

    // A 端记账（经壳层公开命令：写入接缝 + op 产出随之发生）。
    let acc_id = accounts::create_account(
        app_a.state(),
        app_a.clone(),
        tauri_app_lib::accounts::AccountInput {
            name: "现金".into(),
            kind: tauri_app_lib::accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
        },
    )
    .await
    .expect("A 建户应成功");
    let a_txn = transactions::create_transaction(
        app_a.state(),
        app_a.clone(),
        expense_input(&acc_id, 10_000, "A 桌面记的账"),
    )
    .await
    .expect("A 记账应成功");

    // A 同步：发布自己流（明文库 → 明文模式随报告标记）。
    let report_a1 = sync_now(app_a.clone(), None).await.expect("A 首轮应成功");
    assert!(report_a1.uploaded_ops >= 2, "A 应上传自己的 op");
    assert!(report_a1.plaintext_mode, "明文库同步应标记明文模式");

    // B 同步：拉取并重放 A 的流。
    let report_b1 = sync_now(app_b.clone(), None).await.expect("B 首轮应成功");
    assert!(report_b1.applied >= 2, "B 应应用 A 的 op");

    // B 端可观察结果（接线证明）：A 的账户与交易已落 B 库。
    let b_accounts = accounts::list_accounts(app_b.clone().state())
        .await
        .expect("B 账户清单应可读");
    assert!(
        b_accounts.iter().any(|a| a.id == acc_id),
        "A 的账户应已同步到 B"
    );
    let b_conn = app_b.state::<DbState>().conn.clone();
    let b_amount = read_scalar_i64(
        &b_conn.lock().unwrap(),
        "SELECT amount_cents FROM transactions WHERE id = ?1",
        [a_txn.as_str()],
    );
    assert_eq!(b_amount, Some(10_000), "A 的交易金额应在 B 端一致");

    // 反向：B 记账 → B 同步上传 → A 同步收下。
    let b_txn = transactions::create_transaction(
        app_b.state(),
        app_b.clone(),
        expense_input(&acc_id, 2_500, "B 桌面记的账"),
    )
    .await
    .expect("B 记账应成功");
    sync_now(app_b.clone(), None).await.expect("B 二轮应成功");
    sync_now(app_a.clone(), None).await.expect("A 二轮应成功");

    let a_conn = app_a.state::<DbState>().conn.clone();
    let a_amount = read_scalar_i64(
        &a_conn.lock().unwrap(),
        "SELECT amount_cents FROM transactions WHERE id = ?1",
        [b_txn.as_str()],
    );
    assert_eq!(a_amount, Some(2_500), "B 的交易金额应在 A 端一致");

    // 幂等续作：重复轮次零变化（全部已知跳过），数据不重复。
    let again = sync_now(app_b.clone(), None).await.expect("B 三轮应成功");
    assert_eq!(again.applied, 0, "重复轮次不应再应用");
    let count = read_scalar_i64(
        &b_conn.lock().unwrap(),
        "SELECT count(*) FROM transactions",
        [],
    );
    assert_eq!(count, Some(2), "两端各一笔，重复同步不产生重复行");
}

/// 密文库同步的信封接线：状态回显加密形态；缺口令报码化错误；显式口令
/// （主口令 = 库口令）轮次成功——「加密模式由壳层按本库加密形态决定」。
#[tokio::test]
async fn encrypted_library_requires_passphrase_and_seals_with_it() {
    isolate_home();
    let stub = spawn_webdav_stub(Some((STUB_USER, STUB_PASS)));
    // 密文库设备：先建明文库（迁移完成）→ 原位转换 → 以口令重开连接挂载
    //（连接在转换后一次性挂载，tauri manage 同型仅首次生效）。
    let (app, dir) = fresh_app("encrypted");
    db::open_db_in(&dir).unwrap(); // 建明文库（连接随即丢弃，文件已迁移）
    let db_path = dir.join(data_location::DB_FILE_NAME);
    enable_encryption_for_file(&db_path, "master-pass").expect("加密转换应成功");
    assert_eq!(
        probe_file_kind(&db_path).unwrap(),
        tauri_app_lib::db::encryption::DbFileKind::Encrypted
    );
    let conn = db::open_connection_with_passphrase(&db_path, "master-pass").expect("密文库应可开");
    app.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
    });
    let app = app.handle().clone();
    configure_channel(&app, &stub.base_url).await;

    // 状态回显加密形态（明文提示与口令引导的依据）。
    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert!(status.library_encrypted, "密文库应回显加密形态");

    // 缺口令：码化错误（错误模板既有 sync-channel.passphrase-required）。
    let err = sync_now(app.clone(), None).await.expect_err("缺口令应被拒");
    assert_code(err, "sync-channel.passphrase-required");

    // 错误口令在封包上传前被拦下（错误口令封出的段对端解不开，段名幂等跳过
    // 会令重传永不发生）：码化错误与密文备份恢复同款合并口径。
    let err = sync_now(app.clone(), Some("wrong-pass".into()))
        .await
        .expect_err("错误口令应被拦下");
    assert_code(err, "encryption.passphrase-incorrect");

    // 显式口令：信封以主口令封包，轮次成功（域单测已证密文可解，此处钉壳层模式选择）。
    let report = sync_now(app.clone(), Some("master-pass".into()))
        .await
        .expect("凭口令轮次应成功");
    assert!(!report.plaintext_mode, "密文库轮次不应标记明文模式");

    // 上次成功同步时刻随成功轮次落库。
    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert!(status.last_sync_at.is_some(), "成功轮次应更新同步时刻");
}

/// 挂起通知命令面（issue #863）：不可重放 op 经手动同步落入挂起队列后，
/// `get_parked_ops` 返回其身份与码化原因，`get_sync_status` 的 `parked_count`
/// 同步反映——「挂起通知可见」的壳三件套形态（参数解包、码化错误、接线证明）。
#[tokio::test]
async fn parked_ops_are_visible_through_command_surface() {
    isolate_home();
    let stub = spawn_webdav_stub(Some((STUB_USER, STUB_PASS)));
    let (app, _dir) = device_app("parked");
    configure_channel(&app, &stub.base_url).await;

    // 空队列：命令回空清单（读路径零副作用）。
    let parked = get_parked_ops(app.clone()).await.expect("挂起清单应可读");
    assert!(parked.is_empty(), "新端无挂起");

    // 对端投递一条引用不存在账户的 op：写段 + 归并 manifest（真实通道路径）。
    deliver_unreplayable_op(&stub.base_url);

    let report = sync_now(app.clone(), None).await.expect("同步应成功");
    assert_eq!(report.parked, 1, "不可重放 op 应挂起");

    // 挂起通知可见（命令面）：身份、码化原因与详情齐备。
    let parked = get_parked_ops(app.clone()).await.expect("挂起清单应可读");
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].op_id, "it-parked-op");
    assert_eq!(parked[0].entity, "transaction");
    assert_eq!(parked[0].entity_id, "it-parked-txn");
    assert!(
        parked[0].code.contains('.'),
        "原因应是稳定错误码，实际 {:?}",
        parked[0].code
    );
    assert!(!parked[0].message.is_empty(), "原因详情不应为空");
    // 插值参数随挂起行出 wire（issue #957）：壳层 `ParkedOpState::from` 是手工
    // 字段搬运，漏字段即前端渲染残缺句；此处钉住 params 与 code 同源到场。
    assert_eq!(parked[0].code, "account.not-found");
    assert_eq!(
        parked[0].params,
        vec!["no-such-account".to_string()],
        "params 应随 wire 携带动态值"
    );
    assert!(!parked[0].parked_at.is_empty(), "挂起时刻应落库");

    // 状态回显同源：parked_count 与清单长度一致。
    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert_eq!(status.parked_count, 1, "挂起数量应回显");
}

/// 对端投递一条引用不存在账户的 op（段 + manifest 两条真实通道写）。
///
/// 走独立 OS 线程：reqwest 阻塞客户端在 tokio 运行时内构造/析构会 panic
///（「Cannot drop a runtime in a context where blocking is not allowed」）——
/// 与产品侧把阻塞 IO 放进阻塞线程池同一语义（ADR-0069 / 壳层 `sync_now` 形态）。
fn deliver_unreplayable_op(base_url: &str) {
    let base_url = base_url.to_string();
    std::thread::spawn(move || deliver_unreplayable_op_blocking(&base_url))
        .join()
        .expect("对端投递线程不应 panic");
}

fn deliver_unreplayable_op_blocking(base_url: &str) {
    use tauri_app_lib::sync_engine::{
        ChannelLayout, DomainCommand, SyncOp, WebDavConfig, WebDavTransport,
    };
    use tauri_app_lib::test_support::publish_raw_segment;
    use tauri_app_lib::transaction::{NormalizedTransaction, TransactionCommand};

    let transport = WebDavTransport::new(WebDavConfig {
        base_url: base_url.to_string(),
        username: STUB_USER.into(),
        password: STUB_PASS.into(),
    })
    .unwrap();
    let layout = ChannelLayout::new("family").unwrap();
    // schema 版本取产品单点（`db::schema_version`），与重放端同版：走重放路径
    // 而非 schema 偏斜挂起。
    let schema_version = db::schema_version(&tauri_app_lib::test_support::open()).unwrap();
    let op = SyncOp {
        op_id: "it-parked-op".into(),
        device_id: "it-peer-device".into(),
        clock: 1,
        schema_version,
        command: DomainCommand::Transaction(TransactionCommand::Create {
            id: "it-parked-txn".into(),
            row: NormalizedTransaction {
                kind: TransactionKind::Expense,
                amount_cents: 1_000,
                currency_code: "CNY".into(),
                amount_native_cents: 1_000,
                account_id: "no-such-account".into(),
                to_account_id: None,
                funding_account_id: None,
                category_id: None,
                merchant_id: None,
                policy_id: None,
                refund_of_transaction_id: None,
                note: Some("引用不存在账户".into()),
                date: "2026-02-01".into(),
            },
            investment: None,
            split: None,
            convert: None,
        }),
    };
    let payload = vec![op];
    // 段 + manifest 两条真实通道写：线格式成帧收归测试支持域（issue #956），
    // 本处只提供 op 语义与对端身份。
    publish_raw_segment(
        &transport,
        &layout,
        "it-peer-device",
        &EnvelopeMode::Plaintext,
        &payload,
    )
    .unwrap();
}
