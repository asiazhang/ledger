//! 检查点发布/预检/引导命令面集成测试（issue #864 / ADR-0091 决策 9 / ADR-0098
//! 决策 5）。
//!
//! 直调命令函数（`#[tokio::test]`），覆盖壳行为：参数解包、错误码、**双端经
//! 真实 S3 桩「发布检查点 → 新端引导 → 双向增量收敛」整链**（验收判据
//! 「新端经 Checkpoint 引导」「手机记的账在桌面出现、桌面记的账在手机出现」
//! 的自动化形态）与信封形态对齐（密文源端引导后本机转密文、双端互开对方段）。
//! 引导的 SQL 级重建、位点采纳、schema 偏斜归域单测（`sync_engine::tests`，
//! ADR-0087），此处只钉「命令壳 → 通道在位 → 拉取 → 整库换入 → 形态对齐」。

use tauri::Manager;
use tauri_app_lib::commands::accounts;
use tauri_app_lib::commands::sync_channel::{
    bootstrap_sync_from_channel, get_sync_channel_checkpoint, get_sync_status,
    publish_sync_checkpoint, sync_now,
};
use tauri_app_lib::commands::{boot::BootCell, transactions};
use tauri_app_lib::db::data_location;
use tauri_app_lib::db::encryption::{DbFileKind, enable_encryption_for_file, probe_file_kind};
use tauri_app_lib::db::{self, DbState, open_connection_with_passphrase};
use tauri_app_lib::error::AppError;
use tauri_app_lib::test_support::read_scalar_i64;

use crate::isolation::isolate_home;
use crate::sync_channel::{
    configure_channel, device_app, expense_input, fresh_app, spawn_sync_stub,
};

/// 断言码化错误命中的稳定码。
fn assert_code(err: AppError, code: &str) {
    assert!(err.is_code(code), "期望 {code}，实际 {err:?}");
}

/// 在既有目录上重新挂一台「设备」（模拟原位重引导后的新连接）：mock 应用 +
/// 该目录的引导登记态 + 按给定口令打开的库连接（明文库传 `None`）。
fn reattach_app(
    _tag: &str,
    dir: &std::path::Path,
    passphrase: Option<&str>,
) -> tauri::AppHandle<tauri::test::MockRuntime> {
    // 写路径副作用接缝接线（issue #1090）：本helper不经 fresh_app，落库前显式
    // 注册余额刷新实现（幂等，进程级）。
    tauri_app_lib::accounts::balance::install_balance_refresh_hook();
    // 交易域接缝接线（issue #1092 / #1180）：与测试工厂同形——六向实现经组合入口
    // 一次装入（幂等，进程级）。
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
    app.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
    });
    app.handle().clone()
}
/// 铺垫：建户 + 记一笔支出（经壳层公开命令，op 产出随之发生），返回 (账户id, 交易id)。
async fn seed_account_and_expense(
    app: &tauri::AppHandle<tauri::test::MockRuntime>,
    amount_cents: i64,
    note: &str,
) -> (String, String) {
    let acc_id = accounts::create_account(
        app.state(),
        app.clone(),
        tauri_app_lib::accounts::AccountInput {
            name: "现金".into(),
            kind: tauri_app_lib::accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
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

    // 未配置：码化拒绝。
    let err = publish_sync_checkpoint(app.clone(), None)
        .await
        .expect_err("未配置应被拒");
    assert_code(err, "sync-channel.not-configured");

    let stub = spawn_sync_stub();
    configure_channel(&app, &stub);

    // 通道上还没有检查点：预检回 None。
    let precheck = get_sync_channel_checkpoint(app.clone())
        .await
        .expect("预检应成功");
    assert!(precheck.is_none(), "空通道预检应回 None");

    // 发布：代数 1 → 预检可见；再发布：代数推进。
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
async fn dual_device_checkpoint_bootstrap_converges_both_ways() {
    isolate_home();
    let stub = spawn_sync_stub();

    // 桌面端 A（存量数据）：记账 → 同步（op 上通道）→ 发布检查点。
    let (app_a, _dir_a) = device_app("cp-a");
    configure_channel(&app_a, &stub);
    let (acc_id, txn_a) = seed_account_and_expense(&app_a, 10_000, "桌面记的账").await;
    sync_now(app_a.clone(), None).await.expect("A 首轮应成功");
    let published = publish_sync_checkpoint(app_a.clone(), None)
        .await
        .expect("A 发布检查点应成功");

    // 手机端 B（全新空库）：预检 → 引导。
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

    // 验收判据「新端经 Checkpoint 引导」：A 的账户与交易整库就位于 B。
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

    // B 是自己的设备身份；位点表随快照就位（对 A 流有已应用水位，增量轮次
    // 因此只拉位点之后的段；精确水位值归域单测，此处只钉接线）。
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

    // 引导是本机簿记事实重置点：快照携带的来源端「上次同步时刻」不采纳。
    let b_status = get_sync_status(app_b.clone()).await.expect("B 状态应可读");
    assert_eq!(b_status.last_sync_at, None, "引导后上次同步时刻应清零");

    // 重复引导被域守卫拒绝（已参与同步，整库换入会覆盖同步状态）。
    let err = bootstrap_sync_from_channel(app_b.clone(), None)
        .await
        .expect_err("重复引导应被拒");
    assert_code(err, "sync-engine.bootstrap-not-fresh");

    // 桌面记的账在手机出现（增量）：A 再记一笔 → 各自同步 → B 可见。
    let (_, txn_a2) = seed_account_and_expense(&app_a, 2_500, "桌面后记的账").await;
    sync_now(app_a.clone(), None).await.expect("A 二轮应成功");
    sync_now(app_b.clone(), None).await.expect("B 二轮应成功");
    let b_amount2 = read_scalar_i64(
        &b_conn.lock().unwrap(),
        "SELECT amount_cents FROM transactions WHERE id = ?1",
        [txn_a2.as_str()],
    );
    assert_eq!(b_amount2, Some(2_500), "A 的增量交易应到 B");

    // 手机记的账在桌面出现（增量）：B 记一笔 → 各自同步 → A 可见。
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

    // 来源端发布检查点。
    let (app_a, _dir_a) = device_app("guard-a");
    configure_channel(&app_a, &stub);
    let _ = seed_account_and_expense(&app_a, 5_000, "来源端").await;
    sync_now(app_a.clone(), None).await.expect("A 首轮应成功");
    publish_sync_checkpoint(app_a.clone(), None)
        .await
        .expect("A 发布应成功");

    // 引导端已有用户业务数据：「加入即新库」拒绝，快照不下载。经当前代码
    // 写入必产出 op，此处直置清空同步元数据模拟**前同步时代的存量库**
    //（该守卫的真实保护对象：升级前已记帐的旧库；ADR-0086「无公开入口的
    // 库内状态直置」同款）。
    let (app_b, dir_b) = device_app("guard-b");
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
    let _ = dir_b;
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

/// 密文源端 → 明文全新端：引导后本机整库转为密文（后续轮次才开得了通道上的
/// 密文段），重挂连接后两端在密文通道上双向收敛——信封形态全通道一致的接线。
#[tokio::test]
async fn encrypted_source_rekeys_fresh_joiner_and_rounds_stay_interoperable() {
    isolate_home();
    let stub = spawn_sync_stub();
    let master = "master-pass";

    // 源端 A：明文库建好 → 原位转密文 → 按口令重开连接挂载。
    let (app_a, dir_a) = fresh_app("enc-a");
    db::open_db_in(&dir_a).unwrap();
    let db_path_a = dir_a.join(data_location::DB_FILE_NAME);
    enable_encryption_for_file(&db_path_a, master).expect("A 加密转换应成功");
    let conn_a = open_connection_with_passphrase(&db_path_a, master).expect("A 密文库应可开");
    app_a.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn_a)),
    });
    let app_a = app_a.handle().clone();
    configure_channel(&app_a, &stub);
    let (acc_id, _) = seed_account_and_expense(&app_a, 10_000, "密文源端").await;

    // 缺口令发布被拦（复用 resolve_passphrase 单点）；凭口令发布成功。
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

    // 全新明文端 B：凭口令引导 → 整库换入 + 转为本机密文库。
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

    // 模拟原位重引导：以主口令重开连接挂载，快照业务数据就位。
    let app_b2 = reattach_app("enc-b2", &dir_b, Some(master));
    let b_accounts = accounts::list_accounts(app_b2.state())
        .await
        .expect("B 账户应可读");
    assert!(
        b_accounts.iter().any(|a| a.id == acc_id),
        "快照业务数据应在密文库中"
    );

    // 双向收敛（密文通道）：B 记账 → B 同步 → A 同步可见；A 记账 → A 同步 → B 可见。
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

/// 信封形态对齐守卫：密文本机 × 密文快照但口令不是本机主口令 → 拒绝
/// （两把钥匙会让后续轮次互开失败）；密文本机 × 明文快照 → 拒绝（引导后
/// 本机封的密文段明文对端开不了）。两条守卫都在替换本机数据前判定。
#[tokio::test]
async fn envelope_form_mismatch_guards_reject_before_bootstrap() {
    isolate_home();

    // 场景一：密文快照 × 密文本机、口令不一致 → bootstrap-passphrase-mismatch。
    let stub1 = spawn_sync_stub();
    let (app_a, dir_a) = fresh_app("mm-a");
    db::open_db_in(&dir_a).unwrap();
    let db_path_a = dir_a.join(data_location::DB_FILE_NAME);
    enable_encryption_for_file(&db_path_a, "channel-pass").expect("A 加密转换应成功");
    let conn_a = open_connection_with_passphrase(&db_path_a, "channel-pass").unwrap();
    app_a.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn_a)),
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
    app_b.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn_b)),
    });
    let app_b = app_b.handle().clone();
    configure_channel(&app_b, &stub1);
    let err = bootstrap_sync_from_channel(app_b.clone(), Some("channel-pass".into()))
        .await
        .expect_err("口令不一致应被拒");
    assert_code(err, "sync-channel.bootstrap-passphrase-mismatch");

    // 场景二：明文快照 × 密文本机 → bootstrap-form-mismatch。
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
    app_d.manage(DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(conn_d)),
    });
    let app_d = app_d.handle().clone();
    configure_channel(&app_d, &stub2);
    let err = bootstrap_sync_from_channel(app_d.clone(), None)
        .await
        .expect_err("明文快照 × 密文本机应被拒");
    assert_code(err, "sync-channel.bootstrap-form-mismatch");
}
