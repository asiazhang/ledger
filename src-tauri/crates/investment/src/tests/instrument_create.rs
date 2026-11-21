use rusqlite::{Connection, params};

use super::common::insert_instrument_with_source;
use crate::command::InstrumentCommandRow;
use crate::crud::create_instrument;
use crate::{InstrumentInput, InstrumentType};
use tauri_app_lib::test_support::open;

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

fn market_of(conn: &Connection, symbol: &str) -> Option<String> {
    conn.query_row(
        "SELECT market FROM instruments WHERE symbol=?1",
        params![symbol],
        |r| r.get(0),
    )
    .ok()
}

#[test]
fn create_instrument_rejects_fund_with_non_unknown_market() {
    let conn = open();
    let err = create_instrument(
        &conn,
        InstrumentInput {
            symbol: "某基金组合".into(),
            kind: InstrumentType::Fund,
            name: Some("某基金组合".into()),
            currency_code: "CNY".into(),
            market: Some("sh".into()),
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("instrument.fund-market-forbidden"));
    assert_eq!(
        market_of(&conn, "某基金组合"),
        None,
        "被拒的 fund 创建不应产生标的行"
    );
}

#[test]
fn create_instrument_fund_market_normalizes_to_unknown() {
    let conn = open();
    for (symbol, market) in [("某基金组合A", None), ("某基金组合B", Some("unknown"))] {
        create_instrument(
            &conn,
            InstrumentInput {
                symbol: symbol.into(),
                kind: InstrumentType::Fund,
                name: Some(symbol.into()),
                currency_code: "CNY".into(),
                market: market.map(str::to_string),
            },
        )
        .unwrap();
        assert_eq!(market_of(&conn, symbol).as_deref(), Some("unknown"));
    }
}

#[test]
fn create_instrument_non_fund_keeps_market() {
    let conn = open();
    create_instrument(
        &conn,
        InstrumentInput {
            symbol: "600519".into(),
            kind: InstrumentType::Stock,
            name: Some("贵州茅台".into()),
            currency_code: "CNY".into(),
            market: Some("sh".into()),
        },
    )
    .unwrap();
    assert_eq!(market_of(&conn, "600519").as_deref(), Some("sh"));
}

#[test]
fn replay_protocols_reject_fund_market() {
    let conn = open();
    let row = InstrumentCommandRow {
        symbol: "某基金组合".into(),
        kind: InstrumentType::Fund,
        name: Some("某基金组合".into()),
        currency_code: "CNY".into(),
        market: "sz".into(),
    };
    let err = match super::crud::write_instrument(&conn, "inst-replay", &row) {
        Err(e) => e,
        Ok(_) => panic!("重放 Create 携带 fund 市场应被拒绝"),
    };
    assert_eq!(err.code(), Some("instrument.fund-market-forbidden"));

    let err = super::crud::write_instrument_update(
        &conn,
        "某基金组合",
        InstrumentType::Fund,
        Some("某基金组合"),
        "sh",
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("instrument.fund-market-forbidden"));
    assert_eq!(market_of(&conn, "某基金组合"), None);
}
