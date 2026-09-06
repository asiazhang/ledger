//! 股票创建增强的领域落库接缝（issue #694 / ADR-0081 决策 2）：东财往返路由
//! 判定（真实代码触网 / 北交所与矛盾 market 拒绝 / 非代码形态走通用路径）、
//! 命中落库（权威名称 + 解析市场 + 现价）、降级落库（市场保留、既有行不覆盖）。
//! 全部离线驱动，先例：[`super::fund_add`]。

use crate::investment::{
    InstrumentType, StockCreateRoute, StockQuote, create_stock_degraded, persist_stock_quote,
    route_stock_creation,
};

use super::common::setup_db;

/// 构造一份典型股票行情（价格万分之一元刻度）。
fn quote(code: &str, name: &str, market: &str, price: Option<i64>) -> StockQuote {
    StockQuote {
        code: code.to_string(),
        name: name.to_string(),
        market: market.to_string(),
        price_cents: price,
        price_date: price.map(|_| "2026-09-04".to_string()),
        kind_hint: InstrumentType::Stock,
    }
}

/// 查标的行（symbol + stock 类型定位）：(name, market, currency, source)。
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

/// 查现价行：(price_cents, currency_code, priced_at 非空, nav_date, source)。
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

// ---------------------------------------------------------------------------
// 东财往返路由判定：真实代码触网、边界显式拒绝、兜底不触网
// ---------------------------------------------------------------------------

#[test]
fn routes_resolvable_real_code_to_enhance_with_resolved_market() {
    let route = route_stock_creation(Some("sh"), "600519");
    let StockCreateRoute::Enhance(resolved) = route else {
        panic!("显式一致 market 应路由到增强: {route:?}");
    };
    assert_eq!(resolved.market, "sh");
    assert_eq!(resolved.code, "600519");

    // 缺省 market：按形态推断（深市 + 港股补零归一）后同样路由到增强。
    for (market, code) in [("sz", "000001"), ("hk", "00700")] {
        let StockCreateRoute::Enhance(resolved) = route_stock_creation(None, code) else {
            panic!("缺省 market 的真实代码应路由到增强: {code}");
        };
        assert_eq!(resolved.market, market);
        assert_eq!(resolved.code, code, "港股应左补零归一");
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
fn rejects_real_code_with_conflicting_or_unsupported_market() {
    let route = route_stock_creation(Some("sz"), "600519");
    let StockCreateRoute::Reject(e) = route else {
        panic!("真实代码 + 矛盾 market 应拒绝");
    };
    assert!(e.is_code("stock.market-conflict"));

    // 真实代码 + 本接缝未开放的市场：同样是显式拒绝（错挂市场 = 永无行情的错行）。
    let route = route_stock_creation(Some("nasdaq"), "600519");
    let StockCreateRoute::Reject(e) = route else {
        panic!("真实代码 + 未开放 market 应拒绝");
    };
    assert!(e.is_code("stock.market-unsupported"));
}

#[test]
fn routes_non_code_shapes_to_generic_path() {
    // 名称充代码兜底：无 market 与有 market（用户知道大致市场）都不触网。
    for market in [None, Some("sh")] {
        assert!(
            matches!(
                route_stock_creation(market, "某虚拟标的"),
                StockCreateRoute::Generic
            ),
            "名称充代码应走通用路径: {market:?}"
        );
    }
    // 美股 ticker：本接缝未开放（T4 议题），走通用路径、提交 market 原样保留。
    for market in [None, Some("nasdaq")] {
        assert!(
            matches!(
                route_stock_creation(market, "AAPL"),
                StockCreateRoute::Generic
            ),
            "美股 ticker 在 T4 前应走通用路径: {market:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 东财命中落库：权威名称 + 解析市场 + 现价
// ---------------------------------------------------------------------------

#[test]
fn persists_quote_as_stock_row_with_market_and_price() {
    let conn = setup_db();
    let outcome = persist_stock_quote(
        &conn,
        InstrumentType::Stock,
        &quote("600519", "贵州茅台", "sh", Some(150000)),
    )
    .expect("命中落库应成功");

    let (name, market, currency, source) = stock_row(&conn, "600519");
    assert_eq!(name.as_deref(), Some("贵州茅台"), "应回填东财权威名称");
    assert_eq!(market, "sh", "应落解析市场");
    assert_eq!(currency, "CNY", "币种按市场推导（沪深→人民币）");
    assert_eq!(source, "manual");

    let (price_cents, price_currency, priced, nav_date, price_source) =
        price_row(&conn, &outcome.instrument_id).expect("有最新价应落现价缓存");
    assert_eq!(price_cents, 150000);
    assert_eq!(price_currency, "CNY");
    assert!(priced, "priced_at = 写入时刻（同步通道同口径），应非空");
    assert_eq!(nav_date, None, "nav_date 是场外基金语义，股票恒 None");
    assert_eq!(price_source.as_deref(), Some("eastmoney"));
    assert!(outcome.price_written);
}

#[test]
fn persists_quote_without_price_skips_price_row() {
    let conn = setup_db();
    let outcome = persist_stock_quote(
        &conn,
        InstrumentType::Stock,
        &quote("000001", "平安银行", "sz", None),
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
    let conn = setup_db();
    let outcome = persist_stock_quote(
        &conn,
        InstrumentType::Stock,
        &quote("00700", "腾讯控股", "hk", Some(360500)),
    )
    .expect("应成功");
    let (_, _, currency, _) = stock_row(&conn, "00700");
    assert_eq!(currency, "HKD", "港股市价币种推导为港币");
    let (price_cents, price_currency, ..) = price_row(&conn, &outcome.instrument_id).unwrap();
    assert_eq!(price_cents, 360500);
    assert_eq!(price_currency, "HKD");
    let _ = outcome;
}

// ---------------------------------------------------------------------------
// 降级落库：市场保留（价格通道可达的前提）、既有行不覆盖
// ---------------------------------------------------------------------------

#[test]
fn degraded_creation_preserves_resolved_market() {
    let conn = setup_db();
    let outcome = create_stock_degraded(
        &conn,
        InstrumentType::Stock,
        "sh",
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
    let conn = setup_db();
    create_stock_degraded(&conn, InstrumentType::Stock, "sz", "000001", None)
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
    let conn = setup_db();
    // 第一笔：东财可达 → 权威名称回填 + 落价。
    let first = persist_stock_quote(
        &conn,
        InstrumentType::Stock,
        &quote("600519", "贵州茅台", "sh", Some(150000)),
    )
    .expect("命中落库应成功");
    // 第二笔：东财不可达 + AI 提交另一名称 → 降级复用同一 id，权威名称与现价不动。
    let replay = create_stock_degraded(
        &conn,
        InstrumentType::Stock,
        "sh",
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
        "降级重放不得覆盖东财权威名称"
    );
    assert_eq!(market, "sh");
    assert!(
        price_row(&conn, &first.instrument_id).is_some(),
        "既有现价不被降级重放破坏"
    );
}

#[test]
fn persists_quote_with_submitted_etf_kind_preserves_type() {
    let conn = setup_db();
    // 场内基金段代码 + 调用方按类型提示提交 etf：增强照常生效，类型以提交为准
    //（东财类型提示只在查询端点投影，不在此改写）。
    let outcome = persist_stock_quote(
        &conn,
        InstrumentType::Etf,
        &quote("510300", "沪深300ETF", "sh", Some(398500)),
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
