//! 东财股票行情访问层（issue #693 / ADR-0081 决策 1）：按（市场，代码）实时查询
//! 东财个股行情（权威名称 / 最新价 / 价格日期 / 类型提示），供 stocks 查询端点
//! 与后续创建增强、添加投资标的壳共用（注入桩形态与基金详情获取接缝同构）。
//! 报文解析、类型探测、价格换算与命中挑选为纯函数（fixture 单测钉沪深
//! ETF/LOF/股票已知样本，不依赖真实网络）；网络复用行情 HTTP 层的主机池 /
//! 重试 / 限流。
//!
//! 接口：单点行情 `stock/get`——`secid=<市场前缀>.<代码>`，响应 `data` 为单对象：
//! f57 代码、f58 名称、f43 最新价（按 f59 精度位缩放的整数）、f59 价格小数位
//! （股票 2 位、场内基金/港股 3 位）、f62 类型特征字段、f86 更新时间戳（unix 秒）。
//! `data:null`（代码无效，如港股未补零）按查无此码处理。命中判定 = f57 与请求
//! 归一化代码全等（错前缀 secid 也会返回其他标的，回显全等防错配）。
//!
//! f62 与 f59 为未公开字段，语义可能无声变更（spec #690 Further Notes）——
//! f62 的类型探测隔离在 [`detect_kind_hint`] 单点、f59 的换算隔离在
//! [`price_cents_from_raw`] 单点，注入测试钉住已知样本，漂移时改一处即可。

use serde::Deserialize;

use super::fund::deserialize_flexible_f64;
use super::http::{
    API_HOSTS, Pacer, RetryConfig, STOCK_GET_PATH, build_client, f2_to_price,
    request_json_from_hosts, secid_prefix,
};
use super::incremental::beijing_date;
use crate::error::{AppError, Result};
use crate::investment::{InstrumentType, StockQuote};

/// 单点行情查询字段：最新价 / 代码 / 名称 / 精度位 / 类型特征 / 更新时间戳。
const STOCK_QUOTE_FIELDS: &str = "f43,f57,f58,f59,f62,f86";

/// 单点行情接口整体响应：`data` 为 null（secid 无效）时按查无此码处理。
#[derive(Debug, Deserialize)]
pub(crate) struct StockQuoteResponse {
    #[serde(default)]
    pub(crate) data: Option<StockQuoteData>,
}

/// 单点行情详情对象（stock/get 的 data）。
#[derive(Debug, Deserialize)]
pub(crate) struct StockQuoteData {
    /// 最新价原始值（按 f59 精度缩放的整数；停牌/无有效价为 "-"，按缺省 None）。
    #[serde(rename = "f43", default, deserialize_with = "deserialize_flexible_f64")]
    pub(crate) price_raw: Option<f64>,
    /// 回显代码（命中判定键：与请求归一化代码全等）。
    #[serde(rename = "f57", default)]
    pub(crate) code: String,
    /// 东财权威名称（如「贵州茅台」）。
    #[serde(rename = "f58", default)]
    pub(crate) name: String,
    /// 价格小数位（f59，未公开字段；股票 2 位、场内基金/港股 3 位，2026-09 实测）。
    #[serde(rename = "f59", default, deserialize_with = "deserialize_flexible_f64")]
    pub(crate) precision: Option<f64>,
    /// 类型特征字段（f62，未公开字段；场内基金类恒为 0，2026-09 实测）。
    /// 消费只经 [`detect_kind_hint`] 单点，字段语义漂移时改一处。
    #[serde(rename = "f62", default, deserialize_with = "deserialize_flexible_f64")]
    pub(crate) kind_feature: Option<f64>,
    /// 更新时间戳（unix 秒；"-" 等形态按缺省 None，无有效时间不投影价格日期）。
    #[serde(rename = "f86", default, deserialize_with = "deserialize_timestamp")]
    pub(crate) updated_at: Option<i64>,
}

/// 时间戳字段兼容数字与数字字符串（与基金净值 DWJZ 同策略）；≤0 视为无有效时间。
pub(super) fn deserialize_timestamp<'de, D>(d: D) -> std::result::Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(d)?;
    let raw = match value {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    Ok(raw.filter(|&t| t > 0))
}

/// 东财类型特征字段（stock/get 的 f62）→ 标的类型提示的单点探测
/// （spec #690 测试决策 / ADR-0081）。反向工程实测（2026-09）：场内基金类
/// （ETF/LOF）该字段恒为 0，股票为非零值或缺省。字段未公开、语义可能无声
/// 变更——隔离本单点，fixture 单测钉住沪深 ETF/LOF/股票已知样本；探测漂移时
/// 改本函数一处。误判代价仅类型标签（不影响通道与录入形态），已接受。
pub(crate) fn detect_kind_hint(kind_feature: Option<f64>) -> InstrumentType {
    match kind_feature {
        Some(0.0) => InstrumentType::Etf,
        _ => InstrumentType::Stock,
    }
}

/// 东财最新价原始值（f43，按 f59 精度缩放）→ 万分之一元（ADR-0038 价格刻度）：
/// `price_cents = f43 × 10^(4 − f59)`。精度缺省或越界（1..=4 之外）时按市场
/// 回退（A 股 2 位、港股 3 位，与增量同步换算 [`f2_to_price`] 同口径）——
/// 回退分支只兜异常形态，正常样本恒有合法精度位。
pub(crate) fn price_cents_from_raw(raw: f64, precision: Option<f64>, market: &str) -> i64 {
    match precision {
        Some(p) if (1.0..=4.0).contains(&p) => (raw * 10f64.powi(4 - p as i32)).round() as i64,
        _ => f2_to_price(raw, market),
    }
}

/// 更新时间戳（unix 秒，f86）→ 价格日期（北京日历日 ISO 串）；无有效时间戳为
/// None。行情日历以北京时间为准（先例：[`beijing_date`]，UTC+8 边界由测试钉住）。
pub(crate) fn price_date_from_timestamp(ts: Option<i64>) -> Option<String> {
    ts.filter(|&t| t > 0)
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|utc| beijing_date(utc).format("%Y-%m-%d").to_string())
}

/// 从单点行情响应投影领域 DTO：命中判定 = f57 与请求归一化代码全等且名称非空
/// （stock/get 按 secid 精确查询，但错前缀 secid 也会返回其他标的——如
/// 0.501018 返回深市权证「奇消23B」——回显全等是防错配的关键）；未命中返回
/// None（上层转「查无此码」码化错误）。无有效报价（停牌）时价格为 None，
/// 投影为 null（与基金未公布净值投影 null 同构）。
pub(crate) fn pick_stock_quote(
    resp: StockQuoteResponse,
    market: &str,
    code: &str,
) -> Option<StockQuote> {
    let data = resp.data?;
    if data.code != code || data.name.trim().is_empty() {
        return None;
    }
    Some(StockQuote {
        code: code.to_string(),
        name: data.name.trim().to_string(),
        market: market.to_string(),
        price_cents: data
            .price_raw
            .map(|raw| price_cents_from_raw(raw, data.precision, market)),
        price_date: price_date_from_timestamp(data.updated_at),
        kind_hint: detect_kind_hint(data.kind_feature),
    })
}

/// 按（市场，代码）拉取单点行情。市场须已过投资域形态解析（沪深港，见
/// `investment::stock::resolve_stock_market`）；查无此码返回码化中文错误
///（Invalid → 400），网络失败 / 风控拦截由 HTTP 层重试后上抛（Io → 500）。
pub(super) fn fetch_stock_quote(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    market: &str,
    code: &str,
) -> Result<StockQuote> {
    let prefix = secid_prefix(market)
        .ok_or_else(|| AppError::Invalid(format!("市场 {market} 无法构造行情查询")))?;
    let secid = format!("{prefix}.{code}");
    tracing::debug!(secid, "股票单点行情查询");
    let resp: StockQuoteResponse = request_json_from_hosts(
        client,
        &[("secid", secid.as_str()), ("fields", STOCK_QUOTE_FIELDS)],
        STOCK_GET_PATH,
        API_HOSTS,
        RetryConfig::production(),
        pacer,
        &format!("fetch_stock_quote:{secid}"),
        None,
    )?;
    pick_stock_quote(resp, market, code).ok_or_else(|| {
        AppError::codedp(
            "sync.stock-not-found",
            format!("查无股票代码 {code}，请核对后重试"),
            &[code],
        )
    })
}

/// 生产拉取入口：构建客户端与限流器后执行单次行情查询（不经数据库连接，
/// 供 HTTP 壳在连接锁外完成网络往返，先例：`fetch_fund_detail_production`，
/// 单请求叠加限流冷却重试最长可达分钟级）。
pub fn fetch_stock_quote_production(market: &str, code: &str) -> Result<StockQuote> {
    let client = build_client()?;
    let mut pacer = Pacer::default();
    fetch_stock_quote(&client, &mut pacer, market, code)
}
