//! `instruments.instrument_type` CHECK 字面量 ↔ [`InstrumentType::ALL`]
//! 测试期互核（ADR-0108）：DB CHECK（V002）是已发布迁移的冻结副本，宏够不到
//! ——漂移从静默降为测试红。
//!
//! 断言对准**实际应用的 schema**（`sqlite_master` 读回，非迁移文件文本），
//! 顺序敏感：enum 顺序即 CHECK 字面量顺序即 OpenAPI `enum_values` 顺序。

use crate::investment::InstrumentType;
use crate::test_support::{extract_check_in_literals, open};

/// V002 `instrument_type` CHECK 字面量集与 `ALL` 的 `as_str` 集全等（含顺序）。
#[test]
fn v002_instrument_type_check_literals_match_all() {
    let conn = open();
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='instruments'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let literals = extract_check_in_literals(&sql, "instrument_type")
        .expect("instruments 表应带 CHECK(instrument_type IN (...))");
    let expected: Vec<&str> = InstrumentType::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(literals, expected);
}
