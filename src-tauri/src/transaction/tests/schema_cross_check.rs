//! `transactions.kind` CHECK 字面量 ↔ [`TransactionKind::ALL`] 测试期互核
//! （ADR-0108）：代码侧字符串面已由 `closed_set!` 宏同体派生收口，但 DB CHECK
//! （V001）是已发布迁移的冻结副本，宏够不到——漂移从静默降为测试红。
//!
//! 断言对准**实际应用的 schema**（`sqlite_master` 读回，非迁移文件文本），
//! 顺序敏感：enum 顺序即 CHECK 字面量顺序即 OpenAPI `enum_values` 顺序，
//! 三者同源同序。

use crate::test_support::{extract_check_in_literals, open};
use crate::transaction::TransactionKind;

/// V001 `transactions.kind` CHECK 字面量集与 `ALL` 的 `as_str` 集全等（含顺序）。
#[test]
fn v001_kind_check_literals_match_all() {
    let conn = open();
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='transactions'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let literals =
        extract_check_in_literals(&sql, "kind").expect("transactions 表应带 CHECK(kind IN (...))");
    let expected: Vec<&str> = TransactionKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(literals, expected);
}
