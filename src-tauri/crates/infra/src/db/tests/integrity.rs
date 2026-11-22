use crate::db::check_integrity;

#[test]
fn integrity_failure_reports_coded_error() {
    let conn = tauri_app_lib::test_support::open();
    conn.execute_batch(
        "CREATE TABLE t(a, b); \
         CREATE INDEX i ON t(a); \
         INSERT INTO t VALUES (1, 1), (2, 1); \
         PRAGMA writable_schema=ON; \
         UPDATE sqlite_master SET sql='CREATE UNIQUE INDEX i ON t(b)' WHERE name='i'; \
         PRAGMA schema_version=99; \
         PRAGMA writable_schema=OFF;",
    )
    .unwrap();

    let err = check_integrity(&conn).unwrap_err();
    let wire = serde_json::to_value(&err).unwrap();
    assert_eq!(wire["kind"], "Invalid");
    assert_eq!(wire["code"], "db.integrity-check-failed");
    let detail = wire["params"][0].as_str().unwrap();
    assert!(
        err.to_string().starts_with("数据库完整性检查失败: "),
        "message 逐字保留（ADR-0050）: {err}"
    );
    assert!(
        err.to_string().ends_with(detail),
        "检查结果进 params 且与 message 尾部一致: {err}"
    );
}
