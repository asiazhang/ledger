//! 按代码即拉添加基金（issue #301 / ADR-0038 / ADR-0103）：编排接缝（注入报价
//! 获取 stub，统一注入签名（代码，市场））的落库行为——标的字典行（类型 fund /
//! 市场 unknown / 来源 manual）、现价缓存（净值即价格 + 净值日期 + priced_at =
//! 净值日期）、未取到净值不落现价、非法代码前置拦截、查无此码不产生标的行、
//! 幂等复用。全部离线驱动。

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::investment::Quote;
use crate::investment::add_fund_by_code_with;
use crate::investment::fund::validate_fund_code;
use crate::investment::prices::price_value_to_cents;

use crate::test_support::open;

/// 构造一份典型基金报价（统一载荷，ADR-0103）：净值 1.3180 → 13180 万分之一元；
/// 场外通道的价格日期与净值日期同为净值日期，市场与类型提示是场内成员（缺省）。
fn quote(code: &str, name: &str, fund_class: &str, nav: Option<(f64, &str)>) -> Quote {
    let nav = nav.map(|(nav, date)| (price_value_to_cents(nav), date.to_string()));
    Quote {
        code: code.to_string(),
        name: name.to_string(),
        price_cents: nav.as_ref().map(|(cents, _)| *cents),
        price_date: nav.as_ref().map(|(_, date)| date.clone()),
        market: None,
        kind_hint: None,
        fund_class: Some(fund_class.to_string()),
        nav_date: nav.map(|(_, date)| date),
    }
}

fn stub_fetch_with(quote: Quote) -> impl FnMut(&str, &str) -> Result<Quote> {
    move |_code: &str, _market: &str| Ok(quote.clone())
}

/// 查标的行（symbol + 类型定位）：。
fn instrument_row(conn: &Connection, symbol: &str) -> Option<(String, String, String, String)> {
    conn.query_row(
        "SELECT id, name, market, source FROM instruments WHERE symbol=?1 AND instrument_type='fund'",
        [symbol],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .ok()
}

/// 查现价行：(price_cents, currency_code, priced_at, nav_date, source)。
type PriceRow = (i64, String, String, Option<String>, Option<String>);

fn price_row(conn: &Connection, instrument_id: &str) -> Option<PriceRow> {
    conn.query_row(
        "SELECT price_cents, currency_code, priced_at, nav_date, source FROM market_prices WHERE instrument_id=?1",
        [instrument_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        },
    )
    .ok()
}

#[test]
fn adds_fund_with_nav_and_price_cache() {
    let conn = open();
    let result = add_fund_by_code_with(
        &conn,
        "000001",
        &mut stub_fetch_with(quote(
            "000001",
            "华夏成长混合",
            "混合型-灵活",
            Some((1.318, "2026-08-28")),
        )),
    )
    .unwrap();

    // 标的行：类型 fund、市场 unknown、名称为东财权威名称、来源 manual（ADR-0036）。
    let (id, name, market, source) = instrument_row(&conn, "000001").expect("应建 fund 标的行");
    assert_eq!(id, result.instrument_id);
    assert_eq!(name, "华夏成长混合");
    assert_eq!(market, "unknown");
    assert_eq!(source, "manual");

    // 现价缓存：净值 1.3180 元 = 13180 万分之一元（4 位小数保真，ADR-0038）；
    // 币种人民币；priced_at 与 nav_date 同为净值日期；来源东财（净值数据来源）。
    let (price, currency, priced_at, nav_date, price_source) =
        price_row(&conn, &result.instrument_id).expect("应落现价缓存");
    assert_eq!(price, 13180);
    assert_eq!(currency, "CNY");
    assert_eq!(priced_at, "2026-08-28");
    assert_eq!(nav_date.as_deref(), Some("2026-08-28"));
    assert_eq!(price_source.as_deref(), Some("eastmoney"));

    // 结果回执：回填信息与写入状态（价格失效信号判定依据）。
    assert_eq!(result.symbol, "000001");
    assert_eq!(result.name, "华夏成长混合");
    assert_eq!(result.fund_class, "混合型-灵活");
    assert_eq!(result.nav_cents, Some(13180));
    assert_eq!(result.nav_date.as_deref(), Some("2026-08-28"));
    assert!(result.price_written);
}

#[test]
fn adds_fund_without_nav_only_instrument_row() {
    // 新发基金未公布净值：仍建标的行（名称/分类权威回填），不落现价、
    // price_written=false（IPC 层据此不广播价格失效信号——零变化不广播）。
    let conn = open();
    let result = add_fund_by_code_with(
        &conn,
        "012345",
        &mut stub_fetch_with(quote("012345", "新发基金", "混合型", None)),
    )
    .unwrap();

    assert!(instrument_row(&conn, "012345").is_some());
    assert!(price_row(&conn, &result.instrument_id).is_none());
    assert_eq!(result.nav_cents, None);
    assert_eq!(result.nav_date, None);
    assert!(!result.price_written);
}

#[test]
fn invalid_code_rejected_before_fetch() {
    let conn = open();
    for bad in ["12345", "1234567", "00001a", "000 01", ""] {
        let mut called = 0usize;
        let mut fetch = |_code: &str, _market: &str| -> Result<Quote> {
            called += 1;
            Ok(quote("000001", "华夏成长混合", "混合型", None))
        };
        let err = add_fund_by_code_with(&conn, bad, &mut fetch).unwrap_err();
        assert!(
            matches!(err, AppError::Coded { ref message, .. } if message.contains("6 位数字")),
            "「{bad}」应拒绝"
        );
        assert_eq!(called, 0, "非法代码不应发起网络拉取");
    }
    // 库内无 fund 行产生。
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE instrument_type='fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn validate_fund_code_accepts_six_digits_only() {
    // 接受侧全部契约（载荷 Result<()>，拒绝一切入参的实现会使其变红）。
    assert!(validate_fund_code("000001").is_ok());
    assert!(validate_fund_code("510300").is_ok());
    // 拒绝侧消费码化错误（issue #778）：非 6 位 ASCII 数字（全角数字不算）一律
    // 以 fund.code-invalid 拒绝。
    for bad in ["１２３４５", "000 01", "abc123"] {
        let err = validate_fund_code(bad).unwrap_err();
        assert!(
            err.is_code("fund.code-invalid"),
            "「{bad}」应以 fund.code-invalid 拒绝，实际 {err:?}"
        );
    }
}

#[test]
fn unknown_code_error_propagates_without_instrument_row() {
    // 查无此码：获取函数返回中文 Invalid 错误，上抛给 UI；不产生标的行。
    let conn = open();
    let mut fetch = |_code: &str, _market: &str| -> Result<Quote> {
        Err(AppError::Invalid(
            "查无基金代码 999999，请核对后重试".into(),
        ))
    };
    let err = add_fund_by_code_with(&conn, "999999", &mut fetch).unwrap_err();
    assert!(matches!(err, AppError::Invalid(ref msg) if msg.contains("查无基金代码 999999")));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn re_add_reuses_instrument_row_and_overwrites_price() {
    // （代码，fund）已存在（含 AI 通道建的 manual 行）：复用该标的、更新名称、
    // 现价整行覆盖（净值日期水位随之刷新）；来源不随复用改写（随行终身不变）。
    let conn = open();
    let first = add_fund_by_code_with(
        &conn,
        "000001",
        &mut stub_fetch_with(quote(
            "000001",
            "旧名称",
            "混合型-灵活",
            Some((1.0, "2026-08-01")),
        )),
    )
    .unwrap();
    let second = add_fund_by_code_with(
        &conn,
        "000001",
        &mut stub_fetch_with(quote(
            "000001",
            "华夏成长混合",
            "混合型-灵活",
            Some((1.318, "2026-08-28")),
        )),
    )
    .unwrap();

    assert_eq!(first.instrument_id, second.instrument_id);
    let (name, _market, source) = conn
        .query_row(
            "SELECT name, market, source FROM instruments WHERE id=?1",
            [&first.instrument_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(name, "华夏成长混合");
    assert_eq!(source, "manual");

    let (price, priced_at, nav_date) = conn
        .query_row(
            "SELECT price_cents, priced_at, nav_date FROM market_prices WHERE instrument_id=?1",
            [&first.instrument_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(price, 13180);
    assert_eq!(priced_at, "2026-08-28");
    assert_eq!(nav_date.as_deref(), Some("2026-08-28"));
    // 现价仍只有一行（upsert 覆盖，不产生第二条）。
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
