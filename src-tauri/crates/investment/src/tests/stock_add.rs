//! 「添加投资标的」股票侧录入的领域规则（issue #697 / ADR-0081 / spec #690；
//! 换源 ADR-0130 决策 2 / issue #1567）：录入通道解析（沪/深/港显式市场、美股
//! UI 折叠通道 → 聚合查询）、查询阶段编排（解析在先、单次查询）与识别落库
//!（类型自动识别 = 行情 kind_hint，经创建增强同一落库接缝）。全部离线驱动，
//! 先例：[`super::stock_lookup`] / [`super::fund_add`]。

use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    InstrumentType, Market, Quote, StockRoute, add_stock_instrument_with_quote,
    fetch_stock_quote_for_add, prices::TENCENT_PRICE_SOURCE, resolve_add_stock_channel,
};
use ledger_infra::error::AppError;
use tauri_app_lib::test_support::{block_on, open};

/// 构造行情桩的命中回报：统一报价载荷（ADR-0103）自带（市场，代码，价格，类型提示）。
fn hit_quote(market: Market, code: &str, kind: InstrumentType) -> Quote {
    Quote {
        code: code.to_string(),
        name: format!("权威名称·{code}"),
        price_cents: Some(2325000),
        price_date: Some("2026-09-06".to_string()),
        market: Some(market),
        kind_hint: Some(kind),
        fund_class: None,
        nav_date: None,
        constant_unit_price_cents: None,
        price_source: TENCENT_PRICE_SOURCE,
    }
}

/// 未命中错误（与访问层桩同款码化形态）。
fn miss(code: &str) -> AppError {
    AppError::codedp(
        "sync.stock-not-found",
        format!("查无股票代码 {code}，请核对后重试"),
        &[code],
    )
}

/// 请求轨迹句柄：共享 Rc 引用，断言时 borrow 只读访问。
type RequestTrack = Rc<RefCell<Vec<(String, String)>>>;

/// 记录请求轨迹的桩：请求市场与命中市场全等（沪深港显式市场）或为聚合路由值
/// `us` 且命中市场属美股三市场（行情源不区分交易所）即返回命中行情，否则未命中
/// ——换源后查询恒单只。请求轨迹经共享句柄带出供断言。
fn tracking_fetch(
    hit: Option<(Market, Quote)>,
) -> (
    impl FnMut(&str, StockRoute) -> std::future::Ready<ledger_infra::error::Result<Quote>>,
    RequestTrack,
) {
    let requests: RequestTrack = Rc::new(RefCell::new(Vec::new()));
    let track = Rc::clone(&requests);
    (
        // async 接缝桩（ADR-0125 决策 7 / issue #1413）：应答值为立即就绪的 future。
        // 命中判定与数据源行为同构：请求路由 = 命中市场的聚合投影（消费投资域
        // `as_stock_route` 单点，不自建第二份映射，issue #1673）。
        move |code: &str, route: StockRoute| {
            track
                .borrow_mut()
                .push((route.as_str().to_string(), code.to_string()));
            let expected_route = hit
                .as_ref()
                .map(|(m, _)| *m)
                .and_then(|m| m.as_quote_market())
                .map(|qm| qm.as_stock_route());
            match &hit {
                Some((_, quote)) if expected_route == Some(route) => {
                    std::future::ready(Ok(quote.clone()))
                }
                _ => std::future::ready(Err(miss(code))),
            }
        },
        requests,
    )
}

// ---------------------------------------------------------------------------
// 录入通道解析（市场必选的通道闭集）
// ---------------------------------------------------------------------------

#[test]
fn sh_sz_hk_channels_map_to_explicit_markets() {
    assert_eq!(
        resolve_add_stock_channel("sh").unwrap(),
        Some(crate::QuoteMarket::Sh)
    );
    assert_eq!(
        resolve_add_stock_channel("sz").unwrap(),
        Some(crate::QuoteMarket::Sz)
    );
    assert_eq!(
        resolve_add_stock_channel("hk").unwrap(),
        Some(crate::QuoteMarket::Hk)
    );
}

#[test]
fn us_channel_maps_to_shape_resolution_for_aggregate_query() {
    // 美股是 UI 折叠的通道标签（ADR-0081），精确交易所由行情源自报——通道映射
    // 为 None（按 ticker 形态解析为聚合查询，ADR-0130 决策 2）。
    assert_eq!(resolve_add_stock_channel("us").unwrap(), None);
}

#[test]
fn channel_out_of_closed_set_is_rejected() {
    let err = resolve_add_stock_channel("bj").unwrap_err();
    assert!(
        err.is_code("stock.channel-unsupported"),
        "应为码化拒绝: {err}"
    );
    // 场外基金通道走基金接缝（add_fund_by_code_with），不在股票通道闭集。
    let err = resolve_add_stock_channel("fund").unwrap_err();
    assert!(
        err.is_code("stock.channel-unsupported"),
        "基金通道不入股票闭集: {err}"
    );
}

// ---------------------------------------------------------------------------
// 查询阶段：通道 → 候选解析 → 遍历（未命中继续 / 临时错误上抛）
// ---------------------------------------------------------------------------

#[test]
fn sh_channel_hits_first_candidate_with_normalized_code() {
    let (mut fetch, requests) = tracking_fetch(Some((
        Market::Sh,
        hit_quote(Market::Sh, "600519", InstrumentType::Stock),
    )));
    let quote = block_on(fetch_stock_quote_for_add("sh", "600519", &mut fetch)).unwrap();
    assert_eq!(quote.stock_market().unwrap(), Market::Sh);
    assert_eq!(quote.code, "600519");
    assert_eq!(
        *requests.borrow(),
        vec![("sh".into(), "600519".into())],
        "单候选零遍历"
    );
}

#[test]
fn hk_channel_normalizes_code_before_fetch() {
    let (mut fetch, requests) = tracking_fetch(Some((
        Market::Hk,
        hit_quote(Market::Hk, "00700", InstrumentType::Stock),
    )));
    let quote = block_on(fetch_stock_quote_for_add("hk", "700", &mut fetch)).unwrap();
    assert_eq!(quote.code, "00700", "港股左补零归一后发起查询与落库");
    assert_eq!(*requests.borrow(), vec![("hk".into(), "00700".into())]);
}

#[test]
fn us_channel_hits_in_single_query_with_source_reported_market() {
    let (mut fetch, requests) = tracking_fetch(Some((
        Market::Nasdaq,
        hit_quote(Market::Nasdaq, "AAPL", InstrumentType::Stock),
    )));
    let quote = block_on(fetch_stock_quote_for_add("us", "aapl", &mut fetch)).unwrap();
    assert_eq!(
        quote.stock_market().unwrap(),
        Market::Nasdaq,
        "落库市场取行情源自报的精确交易所"
    );
    assert_eq!(quote.code, "AAPL", "ticker 大写归一");
    assert_eq!(
        *requests.borrow(),
        vec![("us".into(), "AAPL".into())],
        "一次聚合查询即命中，不必遍历三市场（ADR-0130 决策 2）"
    );
}

#[test]
fn all_miss_surfaces_not_found() {
    let (mut fetch, requests) = tracking_fetch(None);
    let err = block_on(fetch_stock_quote_for_add("us", "NOPE", &mut fetch)).unwrap_err();
    assert!(
        err.is_code("sync.stock-not-found"),
        "查无此码显式报错: {err}"
    );
    assert_eq!(requests.borrow().len(), 1, "单查询未命中即报");
}

#[test]
fn explicit_market_conflict_rejects_before_any_fetch() {
    let (mut fetch, requests) = tracking_fetch(None);
    // 沪通道 + 港股形态代码：形态矛盾在候选解析单点拒绝，不发起网络。
    let err = block_on(fetch_stock_quote_for_add("sh", "700", &mut fetch)).unwrap_err();
    assert!(
        err.is_code("stock.market-conflict"),
        "形态矛盾显式 400: {err}"
    );
    assert!(requests.borrow().is_empty(), "拒绝在发起网络前");
}

#[test]
fn beijing_exchange_code_rejects_before_any_fetch() {
    let (mut fetch, requests) = tracking_fetch(None);
    let err = block_on(fetch_stock_quote_for_add("sh", "430047", &mut fetch)).unwrap_err();
    assert!(
        err.is_code("stock.bse-unsupported"),
        "北交所暂不支持: {err}"
    );
    assert!(requests.borrow().is_empty());
}

#[test]
fn temporary_error_surfaced_immediately() {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let track = Rc::clone(&requests);
    let mut fetch = move |code: &str, route: StockRoute| {
        track
            .borrow_mut()
            .push((route.as_str().to_string(), code.to_string()));
        std::future::ready(Err(AppError::Io("行情源临时不可达".into())))
    };
    let err = block_on(fetch_stock_quote_for_add("us", "AAPL", &mut fetch)).unwrap_err();
    assert!(matches!(err, AppError::Io(_)), "临时错误原样上抛");
    assert_eq!(requests.borrow().len(), 1);
}

// ---------------------------------------------------------------------------
// 识别落库：类型 = 行情 kind_hint，经创建增强同一落库接缝
// ---------------------------------------------------------------------------

#[test]
fn etf_kind_hint_persists_etf_row_with_quote_backfill() {
    let conn = open();
    let quote = hit_quote(Market::Sz, "159915", InstrumentType::Etf);
    let result = add_stock_instrument_with_quote(&conn, &quote).unwrap();
    assert_eq!(result.symbol, "159915");
    assert_eq!(result.kind, InstrumentType::Etf, "类型特征识别为 ETF");
    assert_eq!(result.market, "sz");
    assert_eq!(result.currency_code, "CNY");
    assert_eq!(result.price_cents, Some(2325000));
    assert!(result.price_written);
    let row: (String, String, String, String) = conn
        .query_row(
            "SELECT name, market, currency_code, source FROM instruments \
             WHERE symbol='159915' AND instrument_type='etf'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or_else(|_| panic!("etf 标的行应存在"));
    assert_eq!(row.0, "权威名称·159915", "权威名称回填");
    assert_eq!(row.1, "sz");
    assert_eq!(row.2, "CNY");
    assert_eq!(row.3, "manual", "来源 manual（非同步通道）");
    // 现价落库：nav_date 恒 None（股票通道语义，净值日期是场外基金语义）。
    let price: (i64, Option<String>) = conn
        .query_row(
            "SELECT price_cents, nav_date FROM market_prices WHERE instrument_id=?1",
            [&result.instrument_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(price, (2325000, None));
}

#[test]
fn stock_kind_hint_persists_stock_row() {
    let conn = open();
    let quote = hit_quote(Market::Nasdaq, "AAPL", InstrumentType::Stock);
    let result = add_stock_instrument_with_quote(&conn, &quote).unwrap();
    assert_eq!(result.kind, InstrumentType::Stock);
    assert_eq!(result.currency_code, "USD", "美股币种按市场推导");
    let market: String = conn
        .query_row(
            "SELECT market FROM instruments WHERE symbol='AAPL' AND instrument_type='stock'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(market, "nasdaq", "美股落精确交易所市场");
}

#[test]
fn repeat_add_reuses_row_idempotently() {
    let conn = open();
    let quote = hit_quote(Market::Sh, "600519", InstrumentType::Stock);
    let first = add_stock_instrument_with_quote(&conn, &quote).unwrap();
    let second = add_stock_instrument_with_quote(&conn, &quote).unwrap();
    assert_eq!(first.instrument_id, second.instrument_id, "幂等复用同一行");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE symbol='600519'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "不产生字典碎片");
}

#[test]
fn suspended_quote_creates_row_without_price() {
    let conn = open();
    let mut quote = hit_quote(Market::Sh, "600519", InstrumentType::Stock);
    quote.price_cents = None;
    quote.price_date = None;
    let result = add_stock_instrument_with_quote(&conn, &quote).unwrap();
    assert!(!result.price_written, "停牌未取到价：仅建标的");
    assert_eq!(result.price_cents, None);
    let price_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM market_prices WHERE instrument_id=?1",
            [&result.instrument_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(price_count, 0, "无现价行");
}
