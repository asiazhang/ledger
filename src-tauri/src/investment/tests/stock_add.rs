//! 「添加投资标的」股票侧录入的领域规则（issue #697 / ADR-0081 / spec #690）：
//! 录入通道解析（沪/深/港显式市场、美股 UI 折叠通道 → 三市场遍历）、查询阶段
//! 编排（候选解析在先、遍历未命中继续 / 临时错误上抛）与识别落库（类型自动识别
//! = 行情 kind_hint，经创建增强同一落库接缝）。全部离线驱动，先例：
//! [`super::stock_lookup`] / [`super::fund_add`]。

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::AppError;
use crate::investment::{
    InstrumentType, StockQuote, add_stock_instrument_with_quote, fetch_stock_quote_for_add,
    resolve_add_stock_channel,
};
use crate::test_support::open;

/// 构造行情桩的命中回报：行情对象自带（市场，代码，价格，类型提示）。
fn hit_quote(market: &str, code: &str, kind: InstrumentType) -> StockQuote {
    StockQuote {
        code: code.to_string(),
        name: format!("权威名称·{code}"),
        market: market.to_string(),
        price_cents: Some(2325000),
        price_date: Some("2026-09-06".to_string()),
        kind_hint: kind,
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

/// 记录请求轨迹的桩：请求（市场，代码）落在命中市场即返回命中行情，其余行情
/// 候选一律未命中——单候选通道首请求即命中；美股通道命中前的候选按未命中继续
/// （遍历语义随之被驱动）。请求轨迹经共享句柄带出供断言。
fn tracking_fetch(
    hit: Option<(&'static str, StockQuote)>,
) -> (
    impl FnMut(&str, &str) -> crate::error::Result<StockQuote>,
    RequestTrack,
) {
    let requests: RequestTrack = Rc::new(RefCell::new(Vec::new()));
    let track = Rc::clone(&requests);
    (
        move |market: &str, code: &str| {
            track
                .borrow_mut()
                .push((market.to_string(), code.to_string()));
            match &hit {
                Some((hit_market, quote)) if *hit_market == market => Ok(quote.clone()),
                _ => Err(miss(code)),
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
    assert_eq!(resolve_add_stock_channel("sh").unwrap(), Some("sh"));
    assert_eq!(resolve_add_stock_channel("sz").unwrap(), Some("sz"));
    assert_eq!(resolve_add_stock_channel("hk").unwrap(), Some("hk"));
}

#[test]
fn us_channel_maps_to_shape_resolution_for_traversal() {
    // 美股是 UI 折叠的通道标签（ADR-0081），落库精确交易所交由候选遍历——
    // 通道映射为 None（按 ticker 形态遍历三市场）。
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
        "sh",
        hit_quote("sh", "600519", InstrumentType::Stock),
    )));
    let quote = fetch_stock_quote_for_add("sh", "600519", &mut fetch).unwrap();
    assert_eq!(quote.market, "sh");
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
        "hk",
        hit_quote("hk", "00700", InstrumentType::Stock),
    )));
    let quote = fetch_stock_quote_for_add("hk", "700", &mut fetch).unwrap();
    assert_eq!(quote.code, "00700", "港股左补零归一后发起查询与落库");
    assert_eq!(*requests.borrow(), vec![("hk".into(), "00700".into())]);
}

#[test]
fn us_channel_traverses_candidates_until_first_hit() {
    let (mut fetch, requests) = tracking_fetch(Some((
        "amex",
        hit_quote("amex", "AAPL", InstrumentType::Stock),
    )));
    let quote = fetch_stock_quote_for_add("us", "aapl", &mut fetch).unwrap();
    assert_eq!(quote.market, "amex", "落库市场取东财回显的精确交易所");
    assert_eq!(quote.code, "AAPL", "ticker 大写归一");
    assert_eq!(
        *requests.borrow(),
        vec![
            ("nasdaq".into(), "AAPL".into()),
            ("nyse".into(), "AAPL".into()),
            ("amex".into(), "AAPL".into()),
        ]
    );
}

#[test]
fn all_candidates_miss_surfaces_not_found() {
    let (mut fetch, requests) = tracking_fetch(None);
    let err = fetch_stock_quote_for_add("us", "NOPE", &mut fetch).unwrap_err();
    assert!(
        err.is_code("sync.stock-not-found"),
        "全候选未命中报查无此码: {err}"
    );
    assert_eq!(requests.borrow().len(), 3, "美股通道三候选逐一遍历");
}

#[test]
fn explicit_market_conflict_rejects_before_any_fetch() {
    let (mut fetch, requests) = tracking_fetch(None);
    // 沪通道 + 港股形态代码：形态矛盾在候选解析单点拒绝，不发起网络。
    let err = fetch_stock_quote_for_add("sh", "700", &mut fetch).unwrap_err();
    assert!(
        err.is_code("stock.market-conflict"),
        "形态矛盾显式 400: {err}"
    );
    assert!(requests.borrow().is_empty(), "拒绝在发起网络前");
}

#[test]
fn beijing_exchange_code_rejects_before_any_fetch() {
    let (mut fetch, requests) = tracking_fetch(None);
    let err = fetch_stock_quote_for_add("sh", "430047", &mut fetch).unwrap_err();
    assert!(
        err.is_code("stock.bse-unsupported"),
        "北交所暂不支持: {err}"
    );
    assert!(requests.borrow().is_empty());
}

#[test]
fn temporary_error_stops_traversal_immediately() {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let track = Rc::clone(&requests);
    let mut fetch = move |market: &str, code: &str| -> crate::error::Result<StockQuote> {
        track
            .borrow_mut()
            .push((market.to_string(), code.to_string()));
        Err(AppError::Io("东财临时不可达".into()))
    };
    let err = fetch_stock_quote_for_add("us", "AAPL", &mut fetch).unwrap_err();
    assert!(matches!(err, AppError::Io(_)), "临时错误上抛不盲试");
    assert_eq!(requests.borrow().len(), 1, "首个候选即中止，不继续遍历");
}

// ---------------------------------------------------------------------------
// 识别落库：类型 = 行情 kind_hint，经创建增强同一落库接缝
// ---------------------------------------------------------------------------

#[test]
fn etf_kind_hint_persists_etf_row_with_quote_backfill() {
    let conn = open();
    let quote = hit_quote("sz", "159915", InstrumentType::Etf);
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
    let quote = hit_quote("nasdaq", "AAPL", InstrumentType::Stock);
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
    let quote = hit_quote("sh", "600519", InstrumentType::Stock);
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
    let mut quote = hit_quote("sh", "600519", InstrumentType::Stock);
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
