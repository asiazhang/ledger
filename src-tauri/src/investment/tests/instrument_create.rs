//! 标的创建（`crud::create_instrument`，手动/AI 共用核心函数）的来源标记
//! （issue #293 / ADR-0036 决策 2、ADR-0037 决策 4「非同步即手动」）：
//! 新建行标 'manual'；（代码，类型）命中复用既有行时只更新名称/市场，
//! 来源随行终身不变。

use rusqlite::{Connection, params};

use super::common::insert_instrument_with_source;
use crate::investment::crud::create_instrument;
use crate::investment::{InstrumentInput, InstrumentType};
use crate::test_support::open;

fn input(symbol: &str, kind: InstrumentType, name: &str) -> InstrumentInput {
    InstrumentInput {
        symbol: symbol.into(),
        kind,
        name: Some(name.into()),
        currency_code: "CNY".into(),
        market: None,
    }
}

fn source_of(conn: &Connection, symbol: &str) -> String {
    conn.query_row(
        "SELECT source FROM instruments WHERE symbol=?1",
        params![symbol],
        |r| r.get(0),
    )
    .unwrap()
}

/// 核心创建函数新建行来源标记为手动（同步通道才写 'eastmoney'，见 sync 模块测试）。
#[test]
fn create_instrument_marks_new_row_manual() {
    let conn = open();
    let id = create_instrument(
        &conn,
        input("稳稳地幸福", InstrumentType::Other, "稳稳地幸福"),
    )
    .unwrap();
    assert!(!id.is_empty());
    assert_eq!(source_of(&conn, "稳稳地幸福"), "manual");
}

/// upsert 复用分支（同码同类型已存在、名称有变 → 更新名称）不改写来源：
/// 既有行来源保持终身不变（与同步更新分支同语义）。
#[test]
fn create_instrument_reuse_keeps_existing_source() {
    let conn = open();
    insert_instrument_with_source(
        &conn,
        "inst-em",
        "600000",
        "浦发银行",
        "CNY",
        "sh",
        "stock",
        "eastmoney",
    );

    let id = create_instrument(
        &conn,
        input("600000", InstrumentType::Stock, "浦发银行改名"),
    )
    .unwrap();
    assert_eq!(id, "inst-em", "命中既有行应复用其 id");

    let (name, source): (Option<String>, String) = conn
        .query_row(
            "SELECT name, source FROM instruments WHERE id='inst-em'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name.as_deref(), Some("浦发银行改名"));
    assert_eq!(source, "eastmoney", "复用既有行不应覆盖来源");
}
