//! 完整性检查失败的码契约（ADR-0050 码化收口，#1072）。
//!
//! `check_integrity` 是三域共用基础设施（启动引导、备份恢复校验、同步 checkpoint
//! 重建）：`PRAGMA integrity_check` 返回非 `ok` 时报码化参数错误 `db.integrity-check-failed`，
//! `message` 逐字保留、检查结果进 `params`。
//!
//! 造错法：`PRAGMA writable_schema` 直接把索引声明改成同名表另一列的 `UNIQUE`
//! 索引（不改索引 b-tree），bump `schema_version` 让同连接重读 schema——随后的
//! `PRAGMA integrity_check` 即返回「row N missing from index …」而非 `ok`。

use crate::db::check_integrity;

#[test]
fn integrity_failure_reports_coded_error() {
    // 建库两行序经统一测试工厂承载（ADR-0084 决策 7）：本用例只在已迁移的
    // 内存库上加一张自建表造错，不旁路工厂。
    let conn = crate::test_support::open();
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
