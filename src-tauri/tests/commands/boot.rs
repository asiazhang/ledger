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

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::connection::open_connection_in;
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::migrate::BOOT_SCHEMA_AHEAD;
use ledger_infra::db::schema_guard::BOOT_SCHEMA_DRIFT;
use tauri::Manager;
use tauri_app_lib::commands::boot::get_boot_status;

use crate::sync_channel::fresh_app;
use tauri_app_lib::test_support::isolate_home;

#[tokio::test]
async fn boot_status_reports_the_recorded_failure_code() {
    isolate_home();
    let (app, _dir) = fresh_app("boot-status");
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    let handle = app.handle().clone();

    let status = get_boot_status(handle.clone()).expect("状态应可读");
    assert_eq!(status.phase, "ready");
    assert_eq!(status.error_code, None);

    let gate = handle.state::<BootFailureGate>();
    gate.set_failed(Some(BOOT_SCHEMA_DRIFT));
    let status = get_boot_status(handle.clone()).expect("状态应可读");
    assert_eq!(status.phase, "failed");
    assert_eq!(status.error_code.as_deref(), Some("boot.schema-drift"));

    gate.set_failed(None);
    let status = get_boot_status(handle.clone()).expect("状态应可读");
    assert_eq!(status.phase, "failed");
    assert_eq!(status.error_code.as_deref(), Some("boot.db-unreadable"));
}

#[tokio::test]
async fn schema_ahead_db_fails_open_with_coded_error() {
    let dir = std::env::temp_dir().join(format!(
        "ledger-schema-ahead-it-{}",
        ledger_infra::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join(DB_FILE_NAME);
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.pragma_update(None, "user_version", 9999).unwrap();
        drop(rusqlite::Connection::open(dir.join("sync.db")).unwrap());
    }
    let err = open_connection_in(&dir).unwrap_err();
    assert_eq!(err.code(), Some(BOOT_SCHEMA_AHEAD));
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let kept: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 9999, "超前库 user_version 应原样保留");
}
