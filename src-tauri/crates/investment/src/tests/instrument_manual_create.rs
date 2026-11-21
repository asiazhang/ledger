use rusqlite::{Connection, params};

use super::common::insert_instrument_with_source;
use crate::create_instrument_manual;
use crate::{InstrumentInput, InstrumentType};
use tauri_app_lib::test_support::open;

fn input(kind: InstrumentType, symbol: &str, name: Option<&str>) -> InstrumentInput {
    InstrumentInput {
        symbol: symbol.into(),
        kind,
        name: name.map(Into::into),
        currency_code: "CNY".into(),
        market: None,
    }
}

fn count_of(conn: &Connection, kind: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM instruments WHERE instrument_type=?1",
        params![kind],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn manual_create_rejects_stock() {
    let conn = open();
    let err = create_instrument_manual(
        &conn,
        input(InstrumentType::Stock, "600519", Some("贵州茅台")),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("股票类标的不支持手动创建"),
        "错误应说明股票类不可手动创建：{err}"
    );
    assert_eq!(count_of(&conn, "stock"), 0, "股票类不应产生标的行");
}

#[test]
fn manual_create_rejects_fund() {
    let conn = open();
    let err = create_instrument_manual(
        &conn,
        input(InstrumentType::Fund, "000001", Some("华夏成长混合")),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("基金类标的不支持手动创建"),
        "错误应指向「添加基金」入口：{err}"
    );
    assert_eq!(count_of(&conn, "fund"), 0, "基金类不应产生标的行");
}

#[test]
fn manual_create_allows_whitelisted_kinds() {
    let conn = open();
    for (kind, symbol) in [
        (InstrumentType::Bond, "019547"),
        (InstrumentType::Etf, "510300"),
        (InstrumentType::Other, "稳稳地幸福"),
    ] {
        let id = create_instrument_manual(&conn, input(kind, symbol, Some("某标的")))
            .unwrap_or_else(|e| panic!("{symbol} 应在白名单内：{e}"));
        assert!(!id.is_empty());
        let source: String = conn
            .query_row(
                "SELECT source FROM instruments WHERE symbol=?1",
                params![symbol],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "manual");
    }
}

#[test]
fn manual_create_requires_name() {
    let conn = open();
    for name in [None, Some(""), Some("   ")] {
        let err = create_instrument_manual(&conn, input(InstrumentType::Other, "HW-VR", name))
            .unwrap_err();
        assert!(
            err.to_string().contains("名称不能为空"),
            "空名称应被拒绝：{err}"
        );
    }
    assert_eq!(count_of(&conn, "other"), 0, "空名称不应产生标的行");
}

#[test]
fn manual_create_reuse_keeps_existing_source() {
    let conn = open();
    insert_instrument_with_source(
        &conn,
        "inst-em",
        "510300",
        "",
        "CNY",
        "sh",
        "etf",
        "eastmoney",
    );
    let id = create_instrument_manual(
        &conn,
        input(InstrumentType::Etf, "510300", Some("沪深300ETF")),
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
    assert_eq!(name.as_deref(), Some("沪深300ETF"), "复用应更新名称");
    assert_eq!(source, "eastmoney", "复用既有行不应改写来源");
}
