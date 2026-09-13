//! 标的创建（`crud::create_instrument`，手动/AI 共用核心函数）的来源标记
//! （issue #293 / ADR-0036 决策 2、ADR-0037 决策 4「非同步即手动」）：
//! 新建行标 'manual'；（代码，类型）命中复用既有行时只更新名称/市场，
//! 来源随行终身不变。

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

// ---------------------------------------------------------------------------
// fund 市场恒 unknown 守卫（ADR-0038 决策 1 修订 / issue #1194）：不变量对全部
// 创建通道成立，守卫落在标的写入协议（本地创建与同步重放共用）而非逐个入口。
// ---------------------------------------------------------------------------

fn market_of(conn: &Connection, symbol: &str) -> Option<String> {
    conn.query_row(
        "SELECT market FROM instruments WHERE symbol=?1",
        params![symbol],
        |r| r.get(0),
    )
    .ok()
}

/// fund 行携带非 unknown 市场被域守卫码化拒绝，且不产生标的行——这是通用创建
/// 通道（AI/HTTP）原样透传市场的兜底（API 集成测试钉壳层 400 形态）。
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

/// 缺省市场归一到 unknown、显式 unknown 照常建行：守卫只拒绝真实市场值。
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

/// 非 fund 类型不受守卫影响（stock 市场照常落库）。
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

/// 同步重放两协议同款守卫原样生效：Create 携带 fund 市场、Update 改写为 fund
/// 市场均在落库前拒绝，不因「重放」通道而放行（ADR-0091：接缝契约与本地写同待遇）。
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
