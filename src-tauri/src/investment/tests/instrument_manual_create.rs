//! 手动创建入口守卫（`create_instrument_manual`，issue #290 / ADR-0036
//! 决策 3）：类型白名单（债券/ETF/其他）与名称必填收在域的手动创建入口
//! （#401 域归位前住 IPC 命令入口层）；核心创建函数保持通用（AI HTTP 端点五类
//! 全开、名称可选，ADR-0037），不经本守卫。

use rusqlite::{Connection, params};

use super::common::setup_db;
use crate::investment::create_instrument_manual;
use crate::investment::{InstrumentInput, InstrumentType};

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

/// 白名单拦截：股票类明确拒绝（中文报错），不产生标的行。
#[test]
fn manual_create_rejects_stock() {
    let conn = setup_db();
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

/// 白名单拦截：基金类拒绝（fund 唯一创建入口归按代码即拉，issue #301 / ADR-0038）。
#[test]
fn manual_create_rejects_fund() {
    let conn = setup_db();
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

/// 白名单放行：债券/ETF/其他三类经守卫到达核心创建函数（新建行来源 manual）。
#[test]
fn manual_create_allows_whitelisted_kinds() {
    let conn = setup_db();
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

/// 名称必填：空串与纯空白均拒绝（自建标的主身份是名称），不产生标的行。
#[test]
fn manual_create_requires_name() {
    let conn = setup_db();
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

/// 守卫放行后 upsert 语义保持：（代码，类型）命中既有行复用并只更名，来源随行不变
/// （守卫只加拦截，不改写核心创建函数行为）。
#[test]
fn manual_create_reuse_keeps_existing_source() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
         VALUES ('inst-em','510300','etf','','CNY','sh',\
                 '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test','eastmoney')",
        [],
    )
    .unwrap();
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
