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

use ledger_infra::db::data_location;
use ledger_infra::db::encryption::{enable_encryption_for_file, probe_file_kind};
use ledger_infra::db::{self, DbState};
use ledger_infra::error::AppError;
use ledger_infra::settings::{self, SettingKey};
use ledger_sync_engine::{EnvelopeMode, SyncChannelConfig};
use ledger_transaction::{TransactionInput, TransactionKind};
use tauri::Manager;
use tauri_app_lib::commands::accounts;
use tauri_app_lib::commands::sync_channel::{
    SyncChannelConfigInput, get_parked_ops, get_sync_channel_config, get_sync_status,
    set_sync_channel_config, sync_now, test_sync_channel_connection,
};
use tauri_app_lib::commands::{boot::BootCell, transactions};
use tauri_app_lib::test_support::{
    S3Addressing, S3Stub, S3StubConfig, ScratchDir, read_scalar_i64, spawn_s3_stub,
};

use tauri_app_lib::test_support::isolate_home;

pub(crate) fn fresh_app(tag: &str) -> (tauri::App<tauri::test::MockRuntime>, ScratchDir) {
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    ledger_sync_engine::trigger::install_after_write_hook();
    let dir = ScratchDir::new(&format!("syncchannel-it-{tag}"));
    let app = tauri::test::mock_app();
    let boot = data_location::boot(&dir);
    app.manage(BootCell::new(boot));
    (app, dir)
}

pub(crate) fn device_app(tag: &str) -> (tauri::AppHandle<tauri::test::MockRuntime>, ScratchDir) {
    let (app, dir) = fresh_app(tag);
    app.manage(db::open_db_in(&dir).unwrap());
    (app.handle().clone(), dir)
}

fn assert_code(err: AppError, code: &str) {
    assert!(err.is_code(code), "期望 {code}，实际 {err:?}");
}

pub(crate) fn expense_input(account_id: &str, amount_cents: i64, note: &str) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
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
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

pub(crate) fn spawn_sync_stub() -> S3Stub {
    spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle))
}

pub(crate) fn configure_channel(app: &tauri::AppHandle<tauri::test::MockRuntime>, stub: &S3Stub) {
    let config = stub.channel_config("family");
    let conn = app.state::<DbState>().conn.clone();
    let guard = conn.lock().unwrap();
    settings::set(&guard, SettingKey::SyncChannelConfig, &config).expect("通道配置应落库");
}

#[tokio::test]
async fn sync_status_defaults_and_s3_channel_config_roundtrip() {
    isolate_home();
    let (app, _dir) = device_app("status");

    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert!(!status.device_id.is_empty(), "设备标识应非空");
    assert!(!status.channel_configured, "新端通道未配置");
    assert_eq!(status.last_sync_at, None, "从未同步");
    assert_eq!(status.parked_count, 0, "新端无挂起");
    assert!(!status.library_encrypted, "明文库");

    let empty = get_sync_channel_config(app.clone())
        .await
        .expect("未配置回显应可读");
    assert!(!empty.configured);
    assert_eq!(empty.endpoint, "", "未配置回空表单");

    let input = SyncChannelConfigInput {
        endpoint: "https://s3.example.com".into(),
        region: "cn-hangzhou".into(),
        bucket: "ledger".into(),
        prefix: "sync/family".into(),
        access_key: "test-access-key".into(),
        secret_key: "test-secret-key".into(),
        path_style: true,
        space_id: Some("family".into()),
    };
    set_sync_channel_config(app.clone(), input)
        .await
        .expect("S3 配置应保存成功");
    let config = get_sync_channel_config(app.clone())
        .await
        .expect("配置应可读");
    assert!(config.configured);
    assert_eq!(config.endpoint, "https://s3.example.com");
    assert_eq!(config.region, "cn-hangzhou");
    assert_eq!(config.bucket, "ledger");
    assert_eq!(config.prefix, "sync/family");
    assert_eq!(config.access_key, "test-access-key");
    assert_eq!(config.secret_key, "test-secret-key");
    assert!(config.path_style, "寻址方式应回显");
    assert_eq!(config.space_id, "family");

    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert!(status.channel_configured, "保存后通道在位");

    let bad = SyncChannelConfigInput {
        endpoint: "   ".into(),
        ..Default::default()
    };
    let err = set_sync_channel_config(app.clone(), bad)
        .await
        .expect_err("空端点应被拒");
    assert_code(err, "sync-channel.base-url-missing");
    let config = get_sync_channel_config(app.clone())
        .await
        .expect("配置应可读");
    assert_eq!(
        config.endpoint, "https://s3.example.com",
        "坏值未覆盖原配置"
    );
    assert_eq!(config.bucket, "ledger", "坏值未覆盖原配置");
}

#[tokio::test]
async fn legacy_config_with_retired_backend_keys_is_readable() {
    isolate_home();
    let (app, _dir) = device_app("legacy");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().unwrap();
        settings::set(
            &guard,
            SettingKey::SyncChannelConfig,
            &serde_json::json!({
                "backend": "webdav",
                "base_url": "https://dav.example.com/dav/",
                "username": "alice",
                "password": "app-pass",
                "space_id": "family",
            }),
        )
        .expect("老配置应可写入");
    }

    let config = get_sync_channel_config(app.clone())
        .await
        .expect("老配置应可回显");
    assert!(config.configured, "键在位即视为已配置");
    assert_eq!(config.space_id, "family", "已识别字段照常回显");
    assert_eq!(config.endpoint, "", "退役后端字段不映射到任何在册字段");
    assert!(!config.path_style);
}

#[tokio::test]
async fn non_https_endpoint_save_is_rejected_and_leaves_config_untouched() {
    isolate_home();
    let (app, _dir) = device_app("insecure");

    set_sync_channel_config(
        app.clone(),
        SyncChannelConfigInput {
            endpoint: "https://s3.example.com".into(),
            region: "cn-hangzhou".into(),
            bucket: "ledger".into(),
            access_key: "test-access-key".into(),
            secret_key: "test-secret-key".into(),
            path_style: true,
            space_id: Some("family".into()),
            ..Default::default()
        },
    )
    .await
    .expect("https 配置应保存成功");

    let insecure_s3 = SyncChannelConfigInput {
        endpoint: "http://127.0.0.1:9000".into(),
        region: "us-east-1".into(),
        bucket: "ledger".into(),
        access_key: "test-access-key".into(),
        secret_key: "test-secret-key".into(),
        path_style: true,
        ..Default::default()
    };
    let err = set_sync_channel_config(app.clone(), insecure_s3)
        .await
        .expect_err("http 端点应被拒");
    assert_code(err, "sync-channel.endpoint-insecure");

    let config = get_sync_channel_config(app.clone())
        .await
        .expect("配置应可读");
    assert_eq!(config.endpoint, "https://s3.example.com");
    assert_eq!(config.bucket, "ledger");
}

#[tokio::test]
async fn sync_now_without_channel_is_rejected_with_coded_error() {
    isolate_home();
    let (app, _dir) = device_app("unconfigured");

    let err = sync_now(app.clone(), None).await.expect_err("未配置应被拒");
    assert_code(err, "sync-channel.not-configured");
}

#[tokio::test]
async fn dual_device_manual_sync_converges_over_real_s3_stub() {
    isolate_home();
    let stub = spawn_sync_stub();
    let (app_a, _dir_a) = device_app("dual-a");
    let (app_b, _dir_b) = device_app("dual-b");
    configure_channel(&app_a, &stub);
    configure_channel(&app_b, &stub);

    let acc_id = accounts::create_account(
        app_a.state(),
        app_a.clone(),
        ledger_accounts::AccountInput {
            name: "现金".into(),
            kind: ledger_accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
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

    let report_a1 = sync_now(app_a.clone(), None).await.expect("A 首轮应成功");
    assert!(report_a1.uploaded_ops >= 2, "A 应上传自己的 op");
    assert!(report_a1.plaintext_mode, "明文库同步应标记明文模式");

    let report_b1 = sync_now(app_b.clone(), None).await.expect("B 首轮应成功");
    assert!(report_b1.applied >= 2, "B 应应用 A 的 op");

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

    let again = sync_now(app_b.clone(), None).await.expect("B 三轮应成功");
    assert_eq!(again.applied, 0, "重复轮次不应再应用");
    let count = read_scalar_i64(
        &b_conn.lock().unwrap(),
        "SELECT count(*) FROM transactions",
        [],
    );
    assert_eq!(count, Some(2), "两端各一笔，重复同步不产生重复行");
}

#[tokio::test]
async fn encrypted_library_requires_passphrase_and_seals_with_it() {
    isolate_home();
    let stub = spawn_sync_stub();
    let (app, dir) = fresh_app("encrypted");
    db::open_db_in(&dir).unwrap();
    let db_path = dir.join(data_location::DB_FILE_NAME);
    enable_encryption_for_file(&db_path, "master-pass").expect("加密转换应成功");
    assert_eq!(
        probe_file_kind(&db_path).unwrap(),
        ledger_infra::db::encryption::DbFileKind::Encrypted
    );
    let conn = db::open_connection_with_passphrase(&db_path, "master-pass").expect("密文库应可开");
    let read_conn = db::open_connection_readonly_with_passphrase(&db_path, "master-pass")
        .expect("密文读连接应可开");
    app.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
        read_conn: std::sync::Arc::new(std::sync::Mutex::new(read_conn)),
    });
    let app = app.handle().clone();
    configure_channel(&app, &stub);

    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert!(status.library_encrypted, "密文库应回显加密形态");

    let err = sync_now(app.clone(), None).await.expect_err("缺口令应被拒");
    assert_code(err, "sync-channel.passphrase-required");

    let err = sync_now(app.clone(), Some("wrong-pass".into()))
        .await
        .expect_err("错误口令应被拦下");
    assert_code(err, "encryption.passphrase-incorrect");

    let report = sync_now(app.clone(), Some("master-pass".into()))
        .await
        .expect("凭口令轮次应成功");
    assert!(!report.plaintext_mode, "密文库轮次不应标记明文模式");

    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert!(status.last_sync_at.is_some(), "成功轮次应更新同步时刻");
}

#[tokio::test]
async fn parked_ops_are_visible_through_command_surface() {
    isolate_home();
    let stub = spawn_sync_stub();
    let (app, _dir) = device_app("parked");
    configure_channel(&app, &stub);

    let parked = get_parked_ops(app.clone()).await.expect("挂起清单应可读");
    assert!(parked.is_empty(), "新端无挂起");

    deliver_unreplayable_op(stub.channel_config("family"));

    let report = sync_now(app.clone(), None).await.expect("同步应成功");
    assert_eq!(report.parked, 1, "不可重放 op 应挂起");

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
    assert_eq!(parked[0].code, "account.not-found");
    assert_eq!(
        parked[0].params,
        vec!["no-such-account".to_string()],
        "params 应随 wire 携带动态值"
    );
    assert!(!parked[0].parked_at.is_empty(), "挂起时刻应落库");

    let status = get_sync_status(app.clone()).await.expect("状态应可读");
    assert_eq!(status.parked_count, 1, "挂起数量应回显");
}

fn deliver_unreplayable_op(config: SyncChannelConfig) {
    std::thread::spawn(move || deliver_unreplayable_op_blocking(&config))
        .join()
        .expect("对端投递线程不应 panic");
}

fn deliver_unreplayable_op_blocking(config: &SyncChannelConfig) {
    use ledger_sync_engine::{ChannelLayout, DomainCommand, SyncOp, build_channel};
    use ledger_transaction::{NormalizedTransaction, TransactionCommand};
    use tauri_app_lib::test_support::publish_raw_segment;

    let channel = build_channel(config).unwrap();
    let layout = ChannelLayout::new("family").unwrap();
    let schema_version = db::schema_version(&tauri_app_lib::test_support::open()).unwrap();
    let op = SyncOp {
        op_id: "it-parked-op".into(),
        device_id: "it-peer-device".into(),
        clock: 1,
        schema_version,
        command: DomainCommand::Transaction(TransactionCommand::Create {
            id: "it-parked-txn".into(),
            row: NormalizedTransaction {
                funding: Vec::new(),
                kind: TransactionKind::Expense,
                amount_cents: 1_000,
                currency_code: "CNY".into(),
                amount_native_cents: 1_000,
                fx_rate_used: None,
                fx_rate_source: None,
                account_id: Some("no-such-account".into()),
                to_account_id: None,
                funding_account_id: None,
                category_id: None,
                merchant_id: None,
                policy_id: None,
                refund_of_transaction_id: None,
                source_order_no: None,
                note: Some("引用不存在账户".into()),
                date: "2026-02-01".into(),
                purchases: Vec::new(),
            },
            investment: None,
            split: None,
            convert: None,
        }),
    };
    let payload = vec![op];
    publish_raw_segment(
        channel.transport(),
        &layout,
        "it-peer-device",
        &EnvelopeMode::Plaintext,
        &payload,
    )
    .unwrap();
}

fn probe_input(endpoint: &str, bucket: &str) -> SyncChannelConfigInput {
    SyncChannelConfigInput {
        endpoint: endpoint.into(),
        region: "us-east-1".into(),
        bucket: bucket.into(),
        access_key: "test-access-key".into(),
        secret_key: "test-secret-key".into(),
        path_style: true,
        space_id: Some("family".into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn test_connection_probes_the_endpoint_and_leaves_saved_config_untouched() {
    isolate_home();
    let (app, _dir) = device_app("probe-wiring");
    set_sync_channel_config(app.clone(), probe_input("https://s3.example.com", "ledger"))
        .await
        .expect("基准配置应保存成功");

    let err = test_sync_channel_connection(probe_input("https://127.0.0.1:1", "ledger"))
        .await
        .expect_err("连不上的端点必须报失败");
    assert_code(err, "sync-channel.network-failed");

    let config = get_sync_channel_config(app.clone())
        .await
        .expect("配置应可读");
    assert_eq!(
        config.endpoint, "https://s3.example.com",
        "探测不得改动已保存配置"
    );
    assert_eq!(config.bucket, "ledger");
}

#[tokio::test]
async fn test_connection_rejects_non_https_endpoint() {
    isolate_home();
    let (_app, _dir) = device_app("probe-insecure");

    let err = test_sync_channel_connection(probe_input("http://127.0.0.1:9000", "ledger"))
        .await
        .expect_err("http 端点应被拒");
    assert_code(err, "sync-channel.endpoint-insecure");
}

use std::time::{Duration, Instant};

async fn wait_for_channel_request(stub: &S3Stub, limit: Duration) {
    let deadline = Instant::now() + limit;
    while stub.requests().is_empty() {
        assert!(
            Instant::now() < deadline,
            "同步应在限时内发出通道请求（manifest 读）"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn sync_now_network_wait_does_not_block_other_commands() {
    isolate_home();
    let (config, gate) = S3StubConfig::new(S3Addressing::PathStyle).gated_requests();
    let stub = spawn_s3_stub(config);
    let (app, _dir) = device_app("sync-lock");
    configure_channel(&app, &stub);

    let acc_id = accounts::create_account(
        app.state(),
        app.clone(),
        ledger_accounts::AccountInput {
            name: "现金".into(),
            kind: ledger_accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .await
    .expect("建户应成功");
    transactions::create_transaction(
        app.state(),
        app.clone(),
        expense_input(&acc_id, 10_000, "同步在途前的账"),
    )
    .await
    .expect("记账应成功");

    let sync = tokio::spawn(sync_now(app.clone(), None));
    wait_for_channel_request(&stub, Duration::from_secs(5)).await;

    let listed =
        tokio::time::timeout(Duration::from_secs(2), accounts::list_accounts(app.state())).await;
    let created = tokio::time::timeout(
        Duration::from_secs(2),
        accounts::create_account(
            app.state(),
            app.clone(),
            ledger_accounts::AccountInput {
                name: "同步在途的账户".into(),
                kind: ledger_accounts::AccountType::Cash,
                currency_code: "CNY".into(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        ),
    )
    .await;

    gate.release(8);
    let report = sync.await.expect("同步任务不应 panic").expect("同步应成功");
    assert!(
        report.uploaded_ops >= 2,
        "本轮应上传铺垫的本机 op，实际 {report:?}"
    );
    listed
        .expect("同步网络在途时读命令应在限时内完成——读连接锁不得被网络等待占据")
        .expect("读命令应成功");
    created
        .expect("同步网络在途时写命令应在限时内完成——写连接锁不得被网络等待占据")
        .expect("写命令应成功");
}

#[tokio::test]
async fn sync_now_reuses_in_flight_round_instead_of_running_a_second() {
    isolate_home();
    let (config, gate) = S3StubConfig::new(S3Addressing::PathStyle).gated_requests();
    let stub = spawn_s3_stub(config);
    let (app, _dir) = device_app("sync-inflight");
    configure_channel(&app, &stub);

    let acc_id = accounts::create_account(
        app.state(),
        app.clone(),
        ledger_accounts::AccountInput {
            name: "现金".into(),
            kind: ledger_accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .await
    .expect("建户应成功");
    transactions::create_transaction(
        app.state(),
        app.clone(),
        expense_input(&acc_id, 10_000, "在途轮次的账"),
    )
    .await
    .expect("记账应成功");

    let first = tokio::spawn(sync_now(app.clone(), None));
    wait_for_channel_request(&stub, Duration::from_secs(5)).await;

    let second = tokio::spawn(sync_now(app.clone(), None));
    let deadline = Instant::now() + Duration::from_millis(400);
    while Instant::now() < deadline {
        assert!(
            stub.requests().len() <= 1,
            "在途时重复触发不得启动第二轮（桩只应看到第一轮的请求），实际 {:?}",
            stub.requests()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    gate.release(8);
    let first_report = first
        .await
        .expect("第一轮任务不应 panic")
        .expect("第一轮应成功");
    let second_report = second
        .await
        .expect("重复触发任务不应 panic")
        .expect("重复触发应成功（复用在途轮次）");
    assert_eq!(first_report, second_report, "两次触发应拿到同一份轮次报告");
}

#[tokio::test]
async fn failed_round_keeps_uploaded_segment_and_success_time() {
    isolate_home();
    let stub = spawn_sync_stub();
    let (app, _dir) = device_app("sync-failure");
    configure_channel(&app, &stub);

    let acc_id = accounts::create_account(
        app.state(),
        app.clone(),
        ledger_accounts::AccountInput {
            name: "现金".into(),
            kind: ledger_accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .await
    .expect("建户应成功");
    transactions::create_transaction(
        app.state(),
        app.clone(),
        expense_input(&acc_id, 10_000, "失败前的账"),
    )
    .await
    .expect("记账应成功");

    let report = sync_now(app.clone(), None).await.expect("首轮应成功");
    assert!(report.uploaded_ops >= 1, "首轮应上传铺垫的本机 op");
    let stamp = get_sync_status(app.clone())
        .await
        .expect("状态应可读")
        .last_sync_at
        .expect("成功轮次应落成功时刻");

    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().unwrap();
        let mut broken = stub.channel_config("family");
        broken.endpoint = "http://127.0.0.1:9".into();
        settings::set(&guard, SettingKey::SyncChannelConfig, &broken).expect("坏配置应落库");
    }
    let device: String = {
        let guard = conn.lock().unwrap();
        guard
            .query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
            .expect("本机设备标识应已生成")
    };
    sync_now(app.clone(), None)
        .await
        .expect_err("不可达通道应让轮次失败");
    assert_eq!(
        get_sync_status(app.clone())
            .await
            .expect("状态应可读")
            .last_sync_at,
        Some(stamp.clone()),
        "失败轮次不得更新成功时刻"
    );

    let segment_puts = |stub: &S3Stub| {
        stub.requests()
            .iter()
            .filter(|r| r.method == "PUT" && r.path.contains(&format!("/streams/{device}/seg-")))
            .count()
    };
    assert_eq!(segment_puts(&stub), 1, "失败轮次不得回滚已上传段");
    {
        let guard = conn.lock().unwrap();
        settings::set(
            &guard,
            SettingKey::SyncChannelConfig,
            &stub.channel_config("family"),
        )
        .expect("恢复配置应落库");
    }
    sync_now(app.clone(), None)
        .await
        .expect("恢复后的轮次应成功");
    assert_eq!(
        segment_puts(&stub),
        1,
        "已上传段重传幂等（同段名跳过），不重复上传"
    );
    assert_ne!(
        get_sync_status(app.clone())
            .await
            .expect("状态应可读")
            .last_sync_at,
        None,
        "恢复后的成功轮次保持成功时刻在位"
    );
}
