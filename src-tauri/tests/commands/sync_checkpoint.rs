use std::time::{Duration, Instant};

use ledger_infra::db::data_location;
use ledger_infra::db::encryption::{DbFileKind, enable_encryption_for_file, probe_file_kind};
use ledger_infra::db::{self, DbState, open_connection_with_passphrase};
use ledger_infra::error::AppError;
use tauri::Manager;
use tauri_app_lib::commands::accounts;
use tauri_app_lib::commands::sync_channel::{
    bootstrap_sync_from_channel, get_sync_channel_checkpoint, get_sync_status,
    publish_sync_checkpoint, sync_now,
};
use tauri_app_lib::commands::{boot::BootCell, transactions};
use tauri_app_lib::test_support::{
    S3Addressing, S3Gate, S3Stub, S3StubConfig, read_scalar_i64, spawn_s3_stub,
};

use crate::sync_channel::{
    configure_channel, device_app, expense_input, fresh_app, spawn_sync_stub,
};
use tauri_app_lib::test_support::isolate_home;

const CHANNEL_REQUEST_LIMIT: Duration = Duration::from_secs(5);
const COMMAND_LIMIT: Duration = Duration::from_secs(2);

fn assert_code(err: AppError, code: &str) {
    assert!(err.is_code(code), "期望 {code}，实际 {err:?}");
}

async fn wait_for_channel_request(stub: &S3Stub, limit: Duration) {
    wait_for_channel_request_after(stub, 0, limit).await;
}

async fn wait_for_channel_request_after(stub: &S3Stub, baseline: usize, limit: Duration) {
    let deadline = Instant::now() + limit;
    while stub.requests().len() <= baseline {
        assert!(
            Instant::now() < deadline,
            "应在限时内发出通道请求（观测到 {baseline} 个后无新请求）"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn drain_through_gate<T>(
    task: tokio::task::JoinHandle<ledger_infra::error::Result<T>>,
    stub: &S3Stub,
    gate: &S3Gate,
    mut released: usize,
) -> ledger_infra::error::Result<T> {
    loop {
        let observed = stub.requests().len();
        if observed > released {
            gate.release(1);
            released += 1;
            continue;
        }
        if task.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let result = task.await.expect("闸门放行任务不应 panic");
    assert_eq!(
        stub.requests().len(),
        released,
        "闸门令牌应逐个放行、不留余量"
    );
    result
}

fn reattach_app(
    _tag: &str,
    dir: &std::path::Path,
    passphrase: Option<&str>,
) -> tauri::AppHandle<tauri::test::MockRuntime> {
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let app = tauri::test::mock_app();
    app.manage(BootCell::new(data_location::boot(dir)));
    let conn = match passphrase {
        Some(passphrase) => {
            db::open_connection_with_passphrase(dir.join(data_location::DB_FILE_NAME), passphrase)
        }
        None => db::open_connection_in(dir),
    }
    .expect("重挂连接应成功");
    let read_conn = match passphrase {
        Some(passphrase) => db::open_connection_readonly_with_passphrase(
            dir.join(data_location::DB_FILE_NAME),
            passphrase,
        ),
        None => db::open_connection_readonly_in(dir),
    }
    .expect("重挂读连接应成功");
    app.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
        read_conn: std::sync::Arc::new(std::sync::Mutex::new(read_conn)),
    });
    app.handle().clone()
}
async fn seed_account_and_expense(
    app: &tauri::AppHandle<tauri::test::MockRuntime>,
    amount_cents: i64,
    note: &str,
) -> (String, String) {
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
    let txn_id = transactions::create_transaction(
        app.state(),
        app.clone(),
        expense_input(&acc_id, amount_cents, note),
    )
    .await
    .expect("记账应成功");
    (acc_id, txn_id)
}

#[tokio::test]
async fn publish_requires_channel_then_advances_generation() {
    isolate_home();
    let (app, _dir) = device_app("publish");

    let err = publish_sync_checkpoint(app.clone(), None)
        .await
        .expect_err("未配置应被拒");
    assert_code(err, "sync-channel.not-configured");
    let err = get_sync_channel_checkpoint(app.clone())
        .await
        .expect_err("未配置预检应被拒");
    assert_code(err, "sync-channel.not-configured");

    let stub = spawn_sync_stub();
    configure_channel(&app, &stub);

    let precheck = get_sync_channel_checkpoint(app.clone())
        .await
        .expect("预检应成功");
    assert!(precheck.is_none(), "空通道预检应回 None");

    let first = publish_sync_checkpoint(app.clone(), None)
        .await
        .expect("发布应成功");
    assert_eq!(first.generation, 1);
    assert!(first.size > 0, "快照体非空");
    assert!(first.plaintext_mode, "明文库发布应标记明文模式");
    let pointer = get_sync_channel_checkpoint(app.clone())
        .await
        .expect("预检应成功")
        .expect("发布后预检应可见");
    assert_eq!(pointer.generation, 1);
    assert_eq!(pointer.size, first.size);

    let second = publish_sync_checkpoint(app.clone(), None)
        .await
        .expect("再发布应成功");
    assert_eq!(second.generation, 2, "代数应单调推进");
}

#[tokio::test]
async fn checkpoint_precheck_network_wait_does_not_block_other_commands() {
    isolate_home();
    let (config, gate) = S3StubConfig::new(S3Addressing::PathStyle).gated_requests();
    let stub = spawn_s3_stub(config);
    let (app, _dir) = device_app("precheck-lock");
    configure_channel(&app, &stub);

    let precheck = tokio::spawn(get_sync_channel_checkpoint(app.clone()));
    wait_for_channel_request(&stub, CHANNEL_REQUEST_LIMIT).await;

    let listed = tokio::time::timeout(COMMAND_LIMIT, accounts::list_accounts(app.state())).await;
    let created = tokio::time::timeout(
        COMMAND_LIMIT,
        accounts::create_account(
            app.state(),
            app.clone(),
            ledger_accounts::AccountInput {
                name: "预检期间的账户".into(),
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

    gate.release(1);
    let precheck = precheck
        .await
        .expect("预检任务不应 panic")
        .expect("预检应成功");
    assert!(precheck.is_none(), "空通道预检应回 None");
    listed
        .expect("预检网络在途时读命令应在限时内完成——读连接锁不得被网络等待占据")
        .expect("读命令应成功");
    created
        .expect("预检网络在途时写命令应在限时内完成——写连接锁不得被网络等待占据")
        .expect("写命令应成功");
}

#[tokio::test]
async fn bootstrap_download_in_flight_keeps_commands_responsive_and_racy_write_errors() {
    isolate_home();
    let (config, gate) = S3StubConfig::new(S3Addressing::PathStyle).gated_requests();
    let stub = spawn_s3_stub(config);

    let (app_a, _dir_a) = device_app("bootstrap-lock-a");
    configure_channel(&app_a, &stub);
    let (source_acc, _txn) = seed_account_and_expense(&app_a, 10_000, "来源端").await;
    drain_through_gate(
        tokio::spawn(publish_sync_checkpoint(app_a.clone(), None)),
        &stub,
        &gate,
        0,
    )
    .await
    .expect("A 发布检查点应成功");

    let (app_b, _dir_b) = device_app("bootstrap-lock-b");
    configure_channel(&app_b, &stub);
    let baseline = stub.requests().len();
    let bootstrap = tokio::spawn(bootstrap_sync_from_channel(app_b.clone(), None));
    wait_for_channel_request_after(&stub, baseline, CHANNEL_REQUEST_LIMIT).await;

    let listed = tokio::time::timeout(COMMAND_LIMIT, accounts::list_accounts(app_b.state())).await;
    let raced = tokio::time::timeout(
        COMMAND_LIMIT,
        accounts::create_account(
            app_b.state(),
            app_b.clone(),
            ledger_accounts::AccountInput {
                name: "下载期间落的账".into(),
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

    gate.release(2);
    let err = bootstrap
        .await
        .expect("引导任务不应 panic")
        .expect_err("下载期间落账的引导应被复验显式拒绝");
    assert_code(err, "sync-engine.bootstrap-not-fresh");

    listed
        .expect("下载在途时读命令应在限时内完成——读路径不得被网络等待占据")
        .expect("读命令应成功");
    let race_acc = raced
        .expect("下载在途时写命令应在限时内完成——写连接锁不得被网络等待占据")
        .expect("写命令应成功");

    let b_accounts = accounts::list_accounts(app_b.state())
        .await
        .expect("B 账户应可读");
    assert!(
        b_accounts.iter().any(|a| a.id == race_acc),
        "下载期间落进的本机写不被快照覆盖（复验收敛为显式报错而非丢账）"
    );
    assert!(
        !b_accounts.iter().any(|a| a.id == source_acc),
        "复验拒绝先于整库换入，快照内容不得就位"
    );
}

#[tokio::test]
async fn checkpoint_publish_network_wait_does_not_block_other_commands() {
    isolate_home();
    let (config, gate) = S3StubConfig::new(S3Addressing::PathStyle).gated_requests();
    let stub = spawn_s3_stub(config);
    let (app, _dir) = device_app("publish-lock");
    configure_channel(&app, &stub);

    let publish = tokio::spawn(publish_sync_checkpoint(app.clone(), None));
    wait_for_channel_request(&stub, CHANNEL_REQUEST_LIMIT).await;

    let listed = tokio::time::timeout(COMMAND_LIMIT, accounts::list_accounts(app.state())).await;
    let created = tokio::time::timeout(
        COMMAND_LIMIT,
        accounts::create_account(
            app.state(),
            app.clone(),
            ledger_accounts::AccountInput {
                name: "发布期间的账户".into(),
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
    let published = publish
        .await
        .expect("发布任务不应 panic")
        .expect("发布应成功");
    assert_eq!(published.generation, 1);
    assert!(published.plaintext_mode, "明文库发布应标记明文模式");
    listed
        .expect("发布网络在途时读命令应在限时内完成——连接锁不得被网络等待占据")
        .expect("读命令应成功");
    created
        .expect("发布网络在途时写命令应在限时内完成——连接锁不得被网络等待占据")
        .expect("写命令应成功");
}

#[tokio::test]
async fn dual_device_checkpoint_bootstrap_converges_both_ways() {
    isolate_home();
    let stub = spawn_sync_stub();

    let (app_a, _dir_a) = device_app("cp-a");
    configure_channel(&app_a, &stub);
    let (acc_id, txn_a) = seed_account_and_expense(&app_a, 10_000, "桌面记的账").await;
    sync_now(app_a.clone(), None).await.expect("A 首轮应成功");
    let published = publish_sync_checkpoint(app_a.clone(), None)
        .await
        .expect("A 发布检查点应成功");

    let (app_b, _dir_b) = device_app("cp-b");
    configure_channel(&app_b, &stub);
    let precheck = get_sync_channel_checkpoint(app_b.clone())
        .await
        .expect("B 预检应成功")
        .expect("通道上应有检查点");
    assert_eq!(precheck.generation, published.generation);

    let outcome = bootstrap_sync_from_channel(app_b.clone(), None)
        .await
        .expect("B 引导应成功");
    assert_eq!(outcome.generation, published.generation);
    assert!(!outcome.reencrypted, "明文快照引导明文库不发生加密转换");

    let b_accounts = accounts::list_accounts(app_b.state())
        .await
        .expect("B 账户应可读");
    assert!(
        b_accounts.iter().any(|a| a.id == acc_id),
        "A 的账户应随快照就位"
    );
    let b_conn = app_b.state::<DbState>().conn.clone();
    let b_amount = read_scalar_i64(
        &b_conn.lock().unwrap(),
        "SELECT amount_cents FROM transactions WHERE id = ?1",
        [txn_a.as_str()],
    );
    assert_eq!(b_amount, Some(10_000), "A 的交易应随快照就位");

    let device_a = get_sync_status(app_a.clone())
        .await
        .expect("A 状态应可读")
        .device_id;
    let device_b = get_sync_status(app_b.clone())
        .await
        .expect("B 状态应可读")
        .device_id;
    assert_ne!(device_a, device_b, "引导端保留自己的设备身份");
    let position_rows = read_scalar_i64(
        &b_conn.lock().unwrap(),
        "SELECT count(*) FROM sync_stream_positions WHERE device_id = ?1",
        [device_a.as_str()],
    );
    assert_eq!(position_rows, Some(1), "A 流位点应随快照就位");

    let b_status = get_sync_status(app_b.clone()).await.expect("B 状态应可读");
    assert_eq!(b_status.last_sync_at, None, "引导后上次同步时刻应清零");

    let err = bootstrap_sync_from_channel(app_b.clone(), None)
        .await
        .expect_err("重复引导应被拒");
    assert_code(err, "sync-engine.bootstrap-not-fresh");

    let (_, txn_a2) = seed_account_and_expense(&app_a, 2_500, "桌面后记的账").await;
    sync_now(app_a.clone(), None).await.expect("A 二轮应成功");
    sync_now(app_b.clone(), None).await.expect("B 二轮应成功");
    let b_amount2 = read_scalar_i64(
        &b_conn.lock().unwrap(),
        "SELECT amount_cents FROM transactions WHERE id = ?1",
        [txn_a2.as_str()],
    );
    assert_eq!(b_amount2, Some(2_500), "A 的增量交易应到 B");

    let (_, txn_b1) = seed_account_and_expense(&app_b, 1_200, "手机记的账").await;
    sync_now(app_b.clone(), None).await.expect("B 三轮应成功");
    sync_now(app_a.clone(), None).await.expect("A 三轮应成功");
    let a_conn = app_a.state::<DbState>().conn.clone();
    let a_amount = read_scalar_i64(
        &a_conn.lock().unwrap(),
        "SELECT amount_cents FROM transactions WHERE id = ?1",
        [txn_b1.as_str()],
    );
    assert_eq!(a_amount, Some(1_200), "B 的交易应到 A");
}

#[tokio::test]
async fn bootstrap_refuses_library_with_user_data() {
    isolate_home();
    let stub = spawn_sync_stub();

    let (app_a, _dir_a) = device_app("guard-a");
    configure_channel(&app_a, &stub);
    let _ = seed_account_and_expense(&app_a, 5_000, "来源端").await;
    sync_now(app_a.clone(), None).await.expect("A 首轮应成功");
    publish_sync_checkpoint(app_a.clone(), None)
        .await
        .expect("A 发布应成功");

    let (app_b, _dir_b) = device_app("guard-b");
    configure_channel(&app_b, &stub);
    let _ = seed_account_and_expense(&app_b, 900, "本机既有数据").await;
    let b_conn = app_b.state::<DbState>().conn.clone();
    b_conn
        .lock()
        .unwrap()
        .execute_batch("DELETE FROM sync_ops; DELETE FROM sync_stream_positions;")
        .unwrap();
    let err = bootstrap_sync_from_channel(app_b.clone(), None)
        .await
        .expect_err("带业务数据的库应被拒");
    assert_code(err, "sync-channel.bootstrap-library-not-empty");
}

#[tokio::test]
async fn bootstrap_without_published_checkpoint_is_coded() {
    isolate_home();
    let stub = spawn_sync_stub();
    let (app_b, _dir_b) = device_app("none-cp");
    configure_channel(&app_b, &stub);
    let err = bootstrap_sync_from_channel(app_b.clone(), None)
        .await
        .expect_err("空通道引导应被拒");
    assert_code(err, "sync-channel.checkpoint-none");
}

#[tokio::test]
async fn encrypted_source_rekeys_fresh_joiner_and_rounds_stay_interoperable() {
    isolate_home();
    let stub = spawn_sync_stub();
    let master = "master-pass";

    let (app_a, dir_a) = fresh_app("enc-a");
    db::open_db_in(&dir_a).unwrap();
    let db_path_a = dir_a.join(data_location::DB_FILE_NAME);
    enable_encryption_for_file(&db_path_a, master).expect("A 加密转换应成功");
    let conn_a = open_connection_with_passphrase(&db_path_a, master).expect("A 密文库应可开");
    let read_a = db::open_connection_readonly_with_passphrase(&db_path_a, master)
        .expect("A 密文读连接应可开");
    app_a.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn_a)),
        read_conn: std::sync::Arc::new(std::sync::Mutex::new(read_a)),
    });
    let app_a = app_a.handle().clone();
    configure_channel(&app_a, &stub);
    let (acc_id, _) = seed_account_and_expense(&app_a, 10_000, "密文源端").await;

    let err = publish_sync_checkpoint(app_a.clone(), None)
        .await
        .expect_err("密文库缺口令发布应被拒");
    assert_code(err, "sync-channel.passphrase-required");
    sync_now(app_a.clone(), Some(master.into()))
        .await
        .expect("A 首轮应成功");
    let published = publish_sync_checkpoint(app_a.clone(), Some(master.into()))
        .await
        .expect("A 凭口令发布应成功");
    assert!(!published.plaintext_mode, "密文库发布不应标记明文模式");

    let (app_b, dir_b) = device_app("enc-b");
    configure_channel(&app_b, &stub);
    let outcome = bootstrap_sync_from_channel(app_b.clone(), Some(master.into()))
        .await
        .expect("B 引导应成功");
    assert!(outcome.reencrypted, "密文快照引导明文库应转换为本机密文库");
    let db_path_b = dir_b.join(data_location::DB_FILE_NAME);
    assert_eq!(
        probe_file_kind(&db_path_b).unwrap(),
        DbFileKind::Encrypted,
        "引导后本机库应为密文形态"
    );

    let app_b2 = reattach_app("enc-b2", &dir_b, Some(master));
    let b_accounts = accounts::list_accounts(app_b2.state())
        .await
        .expect("B 账户应可读");
    assert!(
        b_accounts.iter().any(|a| a.id == acc_id),
        "快照业务数据应在密文库中"
    );

    let (_, txn_b1) = seed_account_and_expense(&app_b2, 3_300, "手机引导后记的账").await;
    sync_now(app_b2.clone(), Some(master.into()))
        .await
        .expect("B 轮次应成功");
    sync_now(app_a.clone(), Some(master.into()))
        .await
        .expect("A 轮次应成功");
    let a_conn = app_a.state::<DbState>().conn.clone();
    let a_amount = read_scalar_i64(
        &a_conn.lock().unwrap(),
        "SELECT amount_cents FROM transactions WHERE id = ?1",
        [txn_b1.as_str()],
    );
    assert_eq!(a_amount, Some(3_300), "B 的交易应到 A（密文段互开）");

    let (_, txn_a2) = seed_account_and_expense(&app_a, 4_400, "桌面后记的账").await;
    sync_now(app_a.clone(), Some(master.into()))
        .await
        .expect("A 轮次应成功");
    sync_now(app_b2.clone(), Some(master.into()))
        .await
        .expect("B 轮次应成功");
    let b_conn = app_b2.state::<DbState>().conn.clone();
    let b_amount = read_scalar_i64(
        &b_conn.lock().unwrap(),
        "SELECT amount_cents FROM transactions WHERE id = ?1",
        [txn_a2.as_str()],
    );
    assert_eq!(b_amount, Some(4_400), "A 的增量应到 B（密文段互开）");
}

#[tokio::test]
async fn envelope_form_mismatch_guards_reject_before_bootstrap() {
    isolate_home();

    let stub1 = spawn_sync_stub();
    let (app_a, dir_a) = fresh_app("mm-a");
    db::open_db_in(&dir_a).unwrap();
    let db_path_a = dir_a.join(data_location::DB_FILE_NAME);
    enable_encryption_for_file(&db_path_a, "channel-pass").expect("A 加密转换应成功");
    let conn_a = open_connection_with_passphrase(&db_path_a, "channel-pass").unwrap();
    let read_a = db::open_connection_readonly_with_passphrase(&db_path_a, "channel-pass").unwrap();
    app_a.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn_a)),
        read_conn: std::sync::Arc::new(std::sync::Mutex::new(read_a)),
    });
    let app_a = app_a.handle().clone();
    configure_channel(&app_a, &stub1);
    let _ = seed_account_and_expense(&app_a, 1_000, "来源端").await;
    sync_now(app_a.clone(), Some("channel-pass".into()))
        .await
        .expect("A 轮次应成功");
    publish_sync_checkpoint(app_a.clone(), Some("channel-pass".into()))
        .await
        .expect("A 发布应成功");

    let (app_b, dir_b) = fresh_app("mm-b");
    db::open_db_in(&dir_b).unwrap();
    let db_path_b = dir_b.join(data_location::DB_FILE_NAME);
    enable_encryption_for_file(&db_path_b, "local-pass").expect("B 加密转换应成功");
    let conn_b = open_connection_with_passphrase(&db_path_b, "local-pass").unwrap();
    let read_b = db::open_connection_readonly_with_passphrase(&db_path_b, "local-pass").unwrap();
    app_b.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn_b)),
        read_conn: std::sync::Arc::new(std::sync::Mutex::new(read_b)),
    });
    let app_b = app_b.handle().clone();
    configure_channel(&app_b, &stub1);
    let err = bootstrap_sync_from_channel(app_b.clone(), Some("channel-pass".into()))
        .await
        .expect_err("口令不一致应被拒");
    assert_code(err, "sync-channel.bootstrap-passphrase-mismatch");

    let stub2 = spawn_sync_stub();
    let (app_c, _dir_c) = device_app("mm-c");
    configure_channel(&app_c, &stub2);
    let _ = seed_account_and_expense(&app_c, 2_000, "明文来源端").await;
    sync_now(app_c.clone(), None).await.expect("C 轮次应成功");
    publish_sync_checkpoint(app_c.clone(), None)
        .await
        .expect("C 发布应成功");

    let (app_d, dir_d) = fresh_app("mm-d");
    db::open_db_in(&dir_d).unwrap();
    let db_path_d = dir_d.join(data_location::DB_FILE_NAME);
    enable_encryption_for_file(&db_path_d, "local-pass").expect("D 加密转换应成功");
    let conn_d = open_connection_with_passphrase(&db_path_d, "local-pass").unwrap();
    let read_d = db::open_connection_readonly_with_passphrase(&db_path_d, "local-pass").unwrap();
    app_d.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn_d)),
        read_conn: std::sync::Arc::new(std::sync::Mutex::new(read_d)),
    });
    let app_d = app_d.handle().clone();
    configure_channel(&app_d, &stub2);
    let err = bootstrap_sync_from_channel(app_d.clone(), None)
        .await
        .expect_err("明文快照 × 密文本机应被拒");
    assert_code(err, "sync-channel.bootstrap-form-mismatch");
}

#[tokio::test]
async fn legacy_single_file_checkpoint_bootstrap_lands_two_dbs_and_converges() {
    use std::sync::{Arc, Mutex};

    use ledger_infra::db::{SYNC_SPLIT_USER_VERSION, migrations, open_connection_unmounted};
    use ledger_sync_engine::{
        ChannelLayout, EnvelopeMode, build_channel, read_ops, stream_positions,
    };
    use tauri_app_lib::test_support::{
        ScratchDir, publish_raw_legacy_checkpoint, publish_raw_segment,
    };

    isolate_home();
    let stub = spawn_sync_stub();
    let _wire = tauri_app_lib::test_support::open();

    let dir_legacy = ScratchDir::new("it-legacy-src");
    let legacy_db = dir_legacy.path().join(data_location::DB_FILE_NAME);
    {
        let mut legacy = open_connection_unmounted(&legacy_db).expect("旧形态裸连接应可开");
        migrations()
            .to_version(&mut legacy, (SYNC_SPLIT_USER_VERSION - 1) as usize)
            .expect("旧形态世界应停在拆库前一代");
    }
    let app_legacy = tauri::test::mock_app();
    app_legacy.manage(BootCell::new(data_location::boot(dir_legacy.path())));
    let conn_legacy = db::open_connection_unmounted(&legacy_db).expect("旧形态写连接应可开");
    let read_legacy = db::open_connection_readonly(&legacy_db).expect("旧形态读连接应可开");
    app_legacy.manage(DbState {
        conn: Arc::new(Mutex::new(conn_legacy)),
        read_conn: Arc::new(Mutex::new(read_legacy)),
    });
    let app_legacy = app_legacy.handle().clone();
    configure_channel(&app_legacy, &stub);

    let legacy_conn = app_legacy.state::<DbState>().conn.clone();
    let acc_id = tauri_app_lib::test_support::seed_account(
        &legacy_conn.lock().unwrap(),
        "acc-legacy",
        "现金",
        "cash",
        "CNY",
        0,
    );
    let txn_a = transactions::create_transaction(
        app_legacy.state(),
        app_legacy.clone(),
        expense_input(&acc_id, 10_000, "旧端快照账"),
    )
    .await
    .expect("旧端记账应成功");
    let dev_src = {
        let guard = legacy_conn.lock().unwrap();
        guard
            .query_row("SELECT id FROM sync_device LIMIT 1", [], |r| {
                r.get::<_, String>(0)
            })
            .expect("旧端设备标识应就位")
    };

    let snapshot_bytes = {
        let guard = legacy_conn.lock().unwrap();
        let snap_path = dir_legacy.path().join("legacy-snapshot.db");
        guard
            .execute(
                "VACUUM INTO ?1",
                rusqlite::params![snap_path.to_string_lossy()],
            )
            .expect("快照应可产出");
        std::fs::read(&snap_path).expect("快照文件应可读")
    };
    let positions = stream_positions(&legacy_conn.lock().unwrap()).expect("位点应可读");
    assert_eq!(positions.len(), 1, "前置：旧端自己的流位点在列");

    let txn_b = transactions::create_transaction(
        app_legacy.state(),
        app_legacy.clone(),
        expense_input(&acc_id, 2_500, "旧端增量账"),
    )
    .await
    .expect("旧端增量记账应成功");
    let source_ops = read_ops(&legacy_conn.lock().unwrap()).expect("旧端日志应可读");
    let increment_ops: Vec<_> = source_ops
        .iter()
        .filter(|op| op.clock > positions[0].applied_through)
        .cloned()
        .collect();
    assert!(
        !increment_ops.is_empty(),
        "前置：快照之后应存在增量 op（增量段非空）"
    );

    let config = stub.channel_config("family");
    let channel = build_channel(&config).expect("通道应可建");
    let transport = channel.transport();
    let layout = ChannelLayout::new("family").unwrap();
    transport
        .ensure_dir(&layout.checkpoint_dir())
        .expect("检查点目录应可建");
    publish_raw_segment(
        transport,
        &layout,
        &dev_src,
        &EnvelopeMode::Plaintext,
        &increment_ops,
    )
    .expect("增量段应可发布");
    let legacy_checkpoint = ledger_sync_engine::Checkpoint {
        positions: positions.clone(),
        snapshot: snapshot_bytes,
        sync_snapshot: Vec::new(),
    };
    let pointer = publish_raw_legacy_checkpoint(
        transport,
        &layout,
        1,
        &EnvelopeMode::Plaintext,
        &legacy_checkpoint,
    )
    .expect("旧形态检查点应可发布");
    assert_eq!(pointer.generation, 1);
    assert!(pointer.sync_file.is_none(), "旧形态指针无 sync_* 字段");

    let (app_b, _dir_b) = device_app("legacy-b");
    configure_channel(&app_b, &stub);
    let outcome = bootstrap_sync_from_channel(app_b.clone(), None)
        .await
        .expect("旧形态快照引导应成功");
    assert_eq!(outcome.generation, 1);

    let b_conn = app_b.state::<DbState>().conn.clone();
    {
        let b_guard = b_conn.lock().unwrap();
        assert_eq!(
            read_scalar_i64(
                &b_guard,
                "SELECT amount_cents FROM transactions WHERE id = ?1",
                [txn_a.as_str()],
            ),
            Some(10_000),
            "旧端快照业务数据应入 main"
        );
        assert_eq!(
            read_scalar_i64(
                &b_guard,
                "SELECT COUNT(*) FROM sync_stream_positions WHERE device_id = ?1",
                [dev_src.as_str()],
            ),
            Some(1),
            "旧端流位点行应随快照就位"
        );
        assert_eq!(
            read_scalar_i64(
                &b_guard,
                "SELECT COUNT(*) FROM main.sqlite_master WHERE type = 'table' AND name = 'sync_ops'",
                [],
            ),
            Some(0),
            "主库不应残留同步表"
        );
    }
    let device_b = get_sync_status(app_b.clone())
        .await
        .expect("B 状态应可读")
        .device_id;
    assert_ne!(device_b, dev_src, "引导端保留自己的设备身份");

    sync_now(app_b.clone(), None).await.expect("B 首轮应成功");
    {
        let b_guard = b_conn.lock().unwrap();
        assert_eq!(
            read_scalar_i64(
                &b_guard,
                "SELECT amount_cents FROM transactions WHERE id = ?1",
                [txn_b.as_str()],
            ),
            Some(2_500),
            "旧端增量交易应到 B"
        );
    }

    let published = publish_sync_checkpoint(app_b.clone(), None)
        .await
        .expect("新端发布检查点应成功");
    assert_eq!(published.generation, 2, "代数自旧形态指针推进");
    let pointer = get_sync_channel_checkpoint(app_b.clone())
        .await
        .expect("预检应成功")
        .expect("通道上应有检查点");
    assert_eq!(pointer.generation, 2);
}
