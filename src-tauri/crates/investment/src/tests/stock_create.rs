use crate::{
    InstrumentType, Market, Quote, StockCreateRoute, StockRoute, adopt_stock_quote,
    create_stock_degraded, prices::TENCENT_PRICE_SOURCE, route_stock_creation,
};

use tauri_app_lib::test_support::open;

fn quote(code: &str, name: &str, market: Market, price: Option<i64>) -> Quote {
    Quote {
        code: code.to_string(),
        name: name.to_string(),
        price_cents: price,
        price_date: price.map(|_| "2026-09-04".to_string()),
        market: Some(market),
        kind_hint: Some(InstrumentType::Stock),
        fund_class: None,
        nav_date: None,
        constant_unit_price_cents: None,
        price_source: TENCENT_PRICE_SOURCE,
    }
}

fn stock_row(
    conn: &rusqlite::Connection,
    symbol: &str,
) -> (Option<String>, String, String, String) {
    conn.query_row(
        "SELECT name, market, currency_code, source FROM instruments \
         WHERE symbol=?1 AND instrument_type='stock'",
        [symbol],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap_or_else(|_| panic!("stock 标的行应存在: {symbol}"))
}

type StockPriceRow = (i64, String, bool, Option<String>, Option<String>);

fn price_row(conn: &rusqlite::Connection, instrument_id: &str) -> Option<StockPriceRow> {
    conn.query_row(
        "SELECT price_cents, currency_code, priced_at IS NOT NULL, nav_date, source \
         FROM market_prices WHERE instrument_id=?1",
        [instrument_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        },
    )
    .ok()
}

#[test]
fn routes_resolvable_real_code_to_enhance_with_resolved_market() {
    let route = route_stock_creation(Some("sh"), "600519");
    let StockCreateRoute::Enhance(plan) = route else {
        panic!("显式一致 market 应路由到增强: {route:?}");
    };
    assert_eq!(plan.candidate.route, StockRoute::Sh);
    assert_eq!(plan.candidate.code, "600519");
    assert_eq!(plan.degrade_market, Market::Sh, "降级保留解析市场");

    for (market, route, code) in [
        (Market::Sz, StockRoute::Sz, "000001"),
        (Market::Hk, StockRoute::Hk, "00700"),
    ] {
        let StockCreateRoute::Enhance(plan) = route_stock_creation(None, code) else {
            panic!("缺省 market 的真实代码应路由到增强: {code}");
        };
        assert_eq!(plan.candidate.route, route);
        assert_eq!(plan.candidate.code, code, "港股应左补零归一");
        assert_eq!(plan.degrade_market, market);
    }
}

#[test]
fn routes_us_ticker_to_enhance_with_single_query() {
    let route = route_stock_creation(None, "aapl");
    let StockCreateRoute::Enhance(plan) = route else {
        panic!("美股 ticker 应路由到增强: {route:?}");
    };
    assert_eq!(
        plan.candidate.route,
        StockRoute::Us,
        "聚合路由值，不落库不出响应"
    );
    assert_eq!(plan.candidate.code, "AAPL", "代码大写归一");
    assert_eq!(plan.degrade_market, Market::Unknown, "缺省降级无法预知市场");

    for market in [Market::Nasdaq, Market::Nyse, Market::Amex] {
        let StockCreateRoute::Enhance(plan) = route_stock_creation(Some(market.as_str()), "AAPL")
        else {
            panic!("显式美股市场应路由到增强: {market}");
        };
        assert_eq!(plan.candidate.route, StockRoute::Us);
        assert_eq!(plan.candidate.code, "AAPL");
        assert_eq!(plan.degrade_market, market);
    }
}

#[test]
fn rejects_beijing_code_before_network() {
    for market in [None, Some("sh"), Some("hk")] {
        let route = route_stock_creation(market, "832000");
        let StockCreateRoute::Reject(e) = route else {
            panic!("北交所代码应显式拒绝: {market:?}");
        };
        assert!(
            e.is_code("stock.bse-unsupported"),
            "应复用查询端点同一码化边界"
        );
    }
}

#[test]
fn rejects_real_code_with_conflicting_market() {
    let route = route_stock_creation(Some("sz"), "600519");
    let StockCreateRoute::Reject(e) = route else {
        panic!("真实代码 + 矛盾 market 应拒绝");
    };
    assert!(e.is_code("stock.market-conflict"));

    let route = route_stock_creation(Some("nasdaq"), "600519");
    let StockCreateRoute::Reject(e) = route else {
        panic!("数字代码 + 美股 market 应拒绝");
    };
    assert!(e.is_code("stock.market-conflict"));

    let route = route_stock_creation(Some("sh"), "AAPL");
    let StockCreateRoute::Reject(e) = route else {
        panic!("字母 ticker + 沪市 market 应拒绝");
    };
    assert!(e.is_code("stock.market-conflict"));
}

#[test]
fn routes_non_code_shapes_to_generic_path() {
    for market in [None, Some("sh")] {
        assert!(
            matches!(
                route_stock_creation(market, "某虚拟标的"),
                StockCreateRoute::Generic
            ),
            "名称充代码应走通用路径: {market:?}"
        );
    }
}

#[test]
fn persists_quote_as_stock_row_with_market_and_price() {
    let conn = open();
    let outcome = adopt_stock_quote(
        &conn,
        InstrumentType::Stock,
        &quote("600519", "贵州茅台", Market::Sh, Some(150000)),
    )
    .expect("命中落库应成功");

    let (name, market, currency, source) = stock_row(&conn, "600519");
    assert_eq!(name.as_deref(), Some("贵州茅台"), "应回填数据源权威名称");
    assert_eq!(market, "sh", "应落解析市场");
    assert_eq!(currency, "CNY", "币种按市场推导（沪深→人民币）");
    assert_eq!(source, "manual");

    let (price_cents, price_currency, priced, nav_date, price_source) =
        price_row(&conn, &outcome.instrument_id).expect("有最新价应落现价缓存");
    assert_eq!(price_cents, 150000);
    assert_eq!(price_currency, "CNY");
    assert!(priced, "priced_at = 写入时刻（同步通道同口径），应非空");
    assert_eq!(nav_date, None, "nav_date 是场外基金语义，股票恒 None");
    assert_eq!(
        price_source.as_deref(),
        Some("tencent"),
        "股票现价来源标记 = 腾讯（ADR-0130 决策 7）"
    );
    assert!(outcome.price_written);
}

#[test]
fn persists_quote_without_price_skips_price_row() {
    let conn = open();
    let outcome = adopt_stock_quote(
        &conn,
        InstrumentType::Stock,
        &quote("000001", "平安银行", Market::Sz, None),
    )
    .expect("应成功");

    let (_, market, currency, _) = stock_row(&conn, "000001");
    assert_eq!(market, "sz");
    assert_eq!(currency, "CNY");
    assert!(
        price_row(&conn, &outcome.instrument_id).is_none(),
        "无价不落现价行"
    );
    assert!(!outcome.price_written);
}

#[test]
fn hong_kong_quote_derives_hkd_currency() {
    let conn = open();
    let outcome = adopt_stock_quote(
        &conn,
        InstrumentType::Stock,
        &quote("00700", "腾讯控股", Market::Hk, Some(360500)),
    )
    .expect("应成功");
    let (_, _, currency, _) = stock_row(&conn, "00700");
    assert_eq!(currency, "HKD", "港股市价币种推导为港币");
    let (price_cents, price_currency, ..) = price_row(&conn, &outcome.instrument_id).unwrap();
    assert_eq!(price_cents, 360500);
    assert_eq!(price_currency, "HKD");
    let _ = outcome;
}

#[test]
fn degraded_creation_preserves_resolved_market() {
    let conn = open();
    let outcome = create_stock_degraded(
        &conn,
        InstrumentType::Stock,
        Market::Sh,
        "600519",
        Some("贵州茅台（账单名）".to_string()),
    )
    .expect("降级建行应成功");

    let (name, market, currency, source) = stock_row(&conn, "600519");
    assert_eq!(
        name.as_deref(),
        Some("贵州茅台（账单名）"),
        "降级行用 AI 提交名称"
    );
    assert_eq!(
        market, "sh",
        "降级必须保留解析市场——行情通道只依赖（市场，代码）"
    );
    assert_eq!(currency, "CNY");
    assert_eq!(source, "manual");
    assert!(
        price_row(&conn, &outcome.instrument_id).is_none(),
        "网络故障无价可落"
    );
    assert!(!outcome.price_written);
}

#[test]
fn degraded_creation_without_ai_name_creates_nameless_row() {
    let conn = open();
    create_stock_degraded(&conn, InstrumentType::Stock, Market::Sz, "000001", None)
        .expect("降级不因缺名称被阻塞");
    let (name, market, ..) = stock_row(&conn, "000001");
    assert!(
        name.is_none(),
        "降级且无 AI 名称时应产生无名称行（行情恢复后可回填）"
    );
    assert_eq!(market, "sz");
}

#[test]
fn degraded_replay_reuses_row_without_overwriting_authoritative_name() {
    let conn = open();
    let first = adopt_stock_quote(
        &conn,
        InstrumentType::Stock,
        &quote("600519", "贵州茅台", Market::Sh, Some(150000)),
    )
    .expect("命中落库应成功");
    let replay = create_stock_degraded(
        &conn,
        InstrumentType::Stock,
        Market::Sh,
        "600519",
        Some("账单抄写名（降级）".to_string()),
    )
    .expect("应成功");
    assert_eq!(
        replay.instrument_id, first.instrument_id,
        "降级重放应幂等复用"
    );
    let (name, market, ..) = stock_row(&conn, "600519");
    assert_eq!(
        name.as_deref(),
        Some("贵州茅台"),
        "降级重放不得覆盖既有权威名称"
    );
    assert_eq!(market, "sh");
    assert!(
        price_row(&conn, &first.instrument_id).is_some(),
        "既有现价不被降级重放破坏"
    );
}

#[test]
fn persists_quote_with_submitted_etf_kind_preserves_type() {
    let conn = open();
    let outcome = adopt_stock_quote(
        &conn,
        InstrumentType::Etf,
        &quote("510300", "沪深300ETF", Market::Sh, Some(398500)),
    )
    .expect("etf 命中落库应成功");

    let (row_type, market, currency): (String, String, String) = conn
        .query_row(
            "SELECT instrument_type, market, currency_code FROM instruments \
             WHERE symbol='510300' AND instrument_type='etf'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("etf 行应存在");
    assert_eq!(row_type, "etf", "类型按提交落库，不被增强改写");
    assert_eq!(market, "sh");
    assert_eq!(currency, "CNY");
    assert!(
        price_row(&conn, &outcome.instrument_id).is_some(),
        "etf 命中同样落现价"
    );
}
