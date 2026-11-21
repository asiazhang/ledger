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
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tauri::Manager;
type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::{self, DbState};
use tauri_app_lib::commands::accounts;
use tauri_app_lib::commands::backup::{create_backup, restore_backup};
use tauri_app_lib::commands::boot::{get_boot_status, restart_app};
use tauri_app_lib::commands::dashboard::dashboard_overview;
use tauri_app_lib::commands::encryption::{enable_encryption, unlock_encryption};
use tauri_app_lib::test_support::isolate_home;

fn readonly_device_app(
    tag: &str,
    locked: bool,
) -> (AppHandle, tauri_app_lib::test_support::ScratchDir) {
    let dir = tauri_app_lib::test_support::ScratchDir::new(&format!("readonly-it-{tag}"));
    let app = tauri::test::mock_app();
    app.manage(tauri_app_lib::commands::boot::BootCell::new(
        db::data_location::boot(&dir),
    ));
    tauri_app_lib::transaction_wiring::install_all();
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(EncryptionGate::new(locked));
    app.manage(BootFailureGate::new());
    (app.handle().clone(), dir)
}

fn app_dir_device_app() -> (tauri::App<tauri::test::MockRuntime>, PathBuf) {
    let app = tauri::test::mock_app();
    let dir = app
        .path()
        .app_data_dir()
        .expect("mock app 应可解析 app_data_dir");
    std::fs::create_dir_all(&dir).unwrap();
    let boot = ledger_infra::db::data_location::boot(&dir);
    app.manage(tauri_app_lib::commands::boot::BootCell::new(boot));
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    (app, dir)
}

async fn seed_account(app: &AppHandle, name: &str, cents: i64) -> String {
    accounts::create_account(
        app.state(),
        app.clone(),
        ledger_accounts::AccountInput {
            name: name.into(),
            kind: ledger_accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(cents),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .await
    .expect("建户应成功")
}

async fn list_accounts_within(app: &AppHandle, limit: Duration) -> Vec<ledger_accounts::Account> {
    tokio::time::timeout(limit, accounts::list_accounts(app.state()))
        .await
        .expect("读命令应在限时内返回（读不得被写者闸门无界阻挡）")
        .expect("读命令应成功（读连接应指向真实库）")
}

#[tokio::test(flavor = "multi_thread")]
async fn read_returns_while_long_write_holds_connection_lock() {
    isolate_home();
    let (app, _dir) = readonly_device_app("ro-blocking", false);
    seed_account(&app, "已提交账户", 12_345).await;

    let write_conn = app.state::<DbState>().conn.clone();
    let (locked_tx, locked_rx) = mpsc::channel::<()>();
    let (read_done_tx, read_done_rx) = mpsc::channel::<()>();
    let writer = std::thread::spawn(move || {
        let conn = write_conn.lock().unwrap();
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        locked_tx.send(()).unwrap();
        let _ = read_done_rx.recv_timeout(Duration::from_secs(10));
        conn.execute_batch("ROLLBACK").unwrap();
    });
    locked_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("写事务应在限时内在途");

    let started = Instant::now();
    let rows = list_accounts_within(&app, Duration::from_secs(2)).await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "读应在写事务在途时即时返回，实际耗时 {:?}",
        started.elapsed()
    );
    assert_eq!(rows.len(), 1, "应读到当前已提交数据（不含在途写入）");
    assert_eq!(rows[0].name, "已提交账户");

    read_done_tx
        .send(())
        .expect("读完成信号应可送达（写线程不应提前收场）");
    writer.join().expect("写事务线程应正常收尾");
}

#[tokio::test(flavor = "multi_thread")]
async fn lazy_write_read_command_stays_on_write_connection() {
    isolate_home();
    let (app, _dir) = readonly_device_app("ro-lazy-write", false);
    let overview = dashboard_overview(app.state()).await.expect("仪表盘应可读");
    let _ = overview;
    seed_account(&app, "缓存失效账户", 999).await;
    dashboard_overview(app.state())
        .await
        .expect("缓存失效后的仪表盘读应成功（甄别路由到写连接，不得走只读连接）");
}

#[tokio::test(flavor = "multi_thread")]
async fn unlock_swaps_in_read_connection_pair() {
    isolate_home();
    let (app, dir) = readonly_device_app("ro-unlock", true);
    let db_path = dir.join(db::data_location::DB_FILE_NAME);
    drop(db::open_db_in(&dir).unwrap());
    ledger_infra::db::encryption::enable_encryption_for_file(&db_path, "主口令").unwrap();
    std::fs::remove_file(db_path.with_extension("db.bak")).unwrap();
    let placeholder_dir = dir.join("placeholder-shape");
    std::fs::create_dir_all(&placeholder_dir).unwrap();
    app.manage(db::open_db_in(&placeholder_dir).unwrap());

    unlock_encryption(app.clone(), "主口令".into())
        .await
        .expect("解锁应成功");

    seed_account(&app, "解锁后账户", 5_000).await;
    let rows = list_accounts_within(&app, Duration::from_secs(2)).await;
    let names: Vec<&str> = rows.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["解锁后账户"], "解锁后读连接应读到真实库数据");
}

#[tokio::test(flavor = "multi_thread")]
async fn file_replacement_swaps_read_conn_out_then_reboot_swaps_pair_back_in() {
    isolate_home();
    let (app, dir) = app_dir_device_app();
    let app = app.handle().clone();
    let db_path = dir.join(ledger_infra::db::data_location::DB_FILE_NAME);
    app.manage(db::open_db_in(&dir).unwrap());

    let target = dir.join("manual-backup.zip");
    seed_account(&app, "备份时账户", 1_000).await;
    create_backup(app.clone(), target.to_string_lossy().into_owned())
        .await
        .expect("备份应成功");
    seed_account(&app, "备份后账户", 2_000).await;

    restore_backup(app.clone(), target.to_string_lossy().into_owned(), None)
        .await
        .expect("恢复应成功");
    let stale_read =
        tokio::time::timeout(Duration::from_secs(2), accounts::list_accounts(app.state()))
            .await
            .expect("换出后读命令应即时返回（不被写者闸门阻挡）");
    assert!(
        stale_read.is_err(),
        "恢复后、重引导前读连接应已换出：读必须报错而非拿到旧库数据，实际 {stale_read:?}"
    );

    restart_app(app.clone()).await.expect("重引导应成功");
    let rows = list_accounts_within(&app, Duration::from_secs(2)).await;
    let names: Vec<&str> = rows.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["备份时账户"],
        "重引导后读连接应读到恢复结果（新库文件成对换入）"
    );

    enable_encryption(app.clone(), "新口令".into())
        .await
        .expect("加密转换应成功");
    assert_eq!(
        ledger_infra::db::encryption::probe_file_kind(&db_path).unwrap(),
        ledger_infra::db::encryption::DbFileKind::Encrypted,
        "转换产物应为密文库"
    );
    let stale_read =
        tokio::time::timeout(Duration::from_secs(2), accounts::list_accounts(app.state()))
            .await
            .expect("换出后读命令应即时返回（不被写者闸门阻挡）");
    assert!(
        stale_read.is_err(),
        "转换后、重引导前读连接应已换出：读必须报错而非拿到旧库数据，实际 {stale_read:?}"
    );

    restart_app(app.clone()).await.expect("重引导应成功");
    let status = get_boot_status(app.clone()).expect("启动状态应可读");
    assert_eq!(status.phase, "locked", "密文库重引导应落解锁屏");
    let locked_read =
        tokio::time::timeout(Duration::from_secs(2), accounts::list_accounts(app.state()))
            .await
            .expect("锁定期间读命令应即时返回");
    assert!(
        locked_read.is_err(),
        "锁定期间读连接是占位形态：读必须报错而非任何库数据，实际 {locked_read:?}"
    );
    unlock_encryption(app.clone(), "新口令".into())
        .await
        .expect("解锁应成功");
    let rows = list_accounts_within(&app, Duration::from_secs(2)).await;
    let names: Vec<&str> = rows.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["备份时账户"],
        "解锁后读连接应读到转换前的原样数据"
    );
}
