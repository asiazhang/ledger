//! `instruments.market` CHECK 字面量 ↔ [`Market::ALL`] 测试期互核（issue #1673，
//! ADR-0108 同款）：DB CHECK（V002）是已发布迁移的冻结副本，宏够不到——漂移
//! 从静默降为测试红。
//!
//! 断言对准**实际应用的 schema**（`sqlite_master` 读回，非迁移文件文本），
//! 顺序敏感：enum 顺序即 CHECK 字面量顺序，两者同源同序。

use crate::market::Market;
use tauri_app_lib::test_support::{extract_check_in_literals, open};

/// V002 `market` CHECK 字面量集与 `ALL` 的 `as_str` 集全等（含顺序）。
#[test]
fn v002_market_check_literals_match_all() {
    let conn = open();
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='instruments'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let literals = extract_check_in_literals(&sql, "market")
        .expect("instruments 表应带 CHECK(market IN (...))");
    let expected: Vec<&str> = Market::ALL.iter().map(|m| m.as_str()).collect();
    assert_eq!(literals, expected);
}
