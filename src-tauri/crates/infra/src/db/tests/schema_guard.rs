use crate::db::schema_guard::{BOOT_SCHEMA_DRIFT, verify_schema};

#[test]
fn healthy_db_passes_schema_guard() {
    let conn = tauri_app_lib::test_support::open();
    assert!(verify_schema(&conn).is_ok(), "健康库不应报漂移");
}

#[test]
fn missing_column_is_detected_as_drift() {
    let conn = tauri_app_lib::test_support::open();
    conn.execute("ALTER TABLE sync_parked_ops DROP COLUMN park_params", [])
        .unwrap();
    let err = verify_schema(&conn).expect_err("缺列库应报漂移");
    assert_eq!(
        err.code(),
        Some(BOOT_SCHEMA_DRIFT),
        "错误码应精确为 boot.schema-drift，实际: {err}"
    );
}

#[test]
fn missing_objects_are_detected_as_drift() {
    let conn = tauri_app_lib::test_support::open();
    conn.execute("DROP INDEX idx_transactions_note_search", [])
        .unwrap();
    conn.execute("DROP VIEW v_holdings", []).unwrap();
    let err = verify_schema(&conn).expect_err("缺对象库应报漂移");
    assert_eq!(
        err.code(),
        Some(BOOT_SCHEMA_DRIFT),
        "错误码应精确为 boot.schema-drift，实际: {err}"
    );
}

#[test]
fn missing_internal_stat_tables_are_tolerated() {
    let conn = tauri_app_lib::test_support::open();
    conn.execute("DROP TABLE IF EXISTS sqlite_stat4", [])
        .unwrap();
    conn.execute("DROP TABLE IF EXISTS sqlite_stat1", [])
        .unwrap();
    assert!(verify_schema(&conn).is_ok(), "缺内部统计表不应误报漂移");
}

#[test]
fn legacy_extra_objects_are_tolerated() {
    let conn = tauri_app_lib::test_support::open();
    conn.execute("CREATE TABLE notes_search (id TEXT, note TEXT)", [])
        .unwrap();
    conn.execute(
        "CREATE INDEX idx_notes_search_text ON notes_search (note)",
        [],
    )
    .unwrap();
    conn.execute("ALTER TABLE merchants ADD COLUMN legacy_col TEXT", [])
        .unwrap();
    assert!(verify_schema(&conn).is_ok(), "遗留多出对象不应误报漂移");
}
