//! 东财基金访问层（issue #301 / ADR-0038 决策 1）：按 6 位基金代码取报价
//! （名称 / 东财分类 / 最新单位净值 + 净值日期），投影为行情接入接缝的统一
//! 载荷 [`Quote`]（ADR-0103 决策 2）；供「按代码即拉」添加基金与 AI 查询/创建
//! 端点（#304 / ADR-0039）复用，即接缝的**查询半边**（按代码取行情，纯取数不落库）。
//! 报文解析与命中挑选为纯函数（fixture 单测，见 `tests/fund_search.rs`），
//! 网络请求复用行情 HTTP 层的主机池 / 重试 / 限流。
//!
//! 接口：基金搜索建议 `FundSearchAPI.ashx`——搜索关键词命中多条（基金 / 股票 /
//! 指数等类别混排），基金条目带 `FundBaseInfo`（含 FCODE / SHORTNAME / FTYPE /
//! DWJZ 单位净值 / FSRQ 净值日期）；同码股票条目 `FundBaseInfo` 为 null。
//! 命中判定 = `FundBaseInfo` 存在且 FCODE 与请求代码全等（基金代码全局唯一）。
//!
//! 该索引只收**在用**基金：已终止（清盘）基金被东财摘出索引，但档案通道（基金
//! 详情页数据文件）仍在服务。搜索未命中时回退档案通道改判「存在」并回填名称与
//! 最后一期单位净值（ADR-0039 修订，issue #1212），两段皆未命中才是查无此码。

use serde::Deserialize;

use super::fund_nav::{FundArchive, PINGZHONG_HOSTS, fetch_fund_archive_from};
use super::http::{Pacer, RetryConfig, build_client, request_json_from_hosts};
use crate::error::{AppError, Result};
use crate::investment::Quote;
use crate::investment::prices::price_value_to_cents;

// 基金搜索建议接口：单主机（无公开镜像池），复用行情层的重试与限流泛型层。
const FUND_SEARCH_HOSTS: &[&str] = &["https://fundsuggest.eastmoney.com"];
const FUND_SEARCH_PATH: &str = "/FundSearch/api/FundSearchAPI.ashx";

/// 基金搜索建议接口整体响应：`Datas` 可能缺省（接口异常形态），按无命中处理。
#[derive(Debug, Deserialize)]
pub(crate) struct FundSearchResponse {
    #[serde(rename = "Datas", default)]
    pub(crate) datas: Option<Vec<FundSearchItem>>,
}

/// 搜索建议单条：基金条目 `FundBaseInfo` 非空；股票 / 指数条目为 null。
#[derive(Debug, Deserialize)]
pub(crate) struct FundSearchItem {
    #[serde(rename = "NAME", default)]
    pub(crate) name: Option<String>,
    #[serde(rename = "FundBaseInfo")]
    pub(crate) fund_base_info: Option<FundBaseInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FundBaseInfo {
    /// 基金代码（全局唯一，命中判定键）。
    #[serde(rename = "FCODE")]
    pub(crate) fcode: String,
    /// 基金简称（如「华夏成长混合」）；接口偶发缺省时回退条目外层 NAME。
    #[serde(rename = "SHORTNAME", default)]
    pub(crate) shortname: Option<String>,
    /// 东财基金分类（如「混合型-灵活」）。
    #[serde(rename = "FTYPE", default)]
    pub(crate) ftype: String,
    /// 最新单位净值（真实价格值，元）：数字或数字字符串，未公布为 null。
    #[serde(
        rename = "DWJZ",
        default,
        deserialize_with = "deserialize_flexible_f64"
    )]
    pub(crate) dwjz: Option<f64>,
    /// 净值日期（ISO 日期）；未公布净值时缺省。
    #[serde(rename = "FSRQ", default)]
    pub(crate) fsrq: Option<String>,
}

/// 数值字段兼容数字与数字字符串两种 wire 形态（DWJZ 两种都出现过）；
/// 非数值（含 null）按缺省处理。历史净值接口（fund_nav）的 DWJZ 同形态，共用。
pub(super) fn deserialize_flexible_f64<'de, D>(d: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(d)?;
    Ok(match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    })
}

/// 从搜索建议响应中挑选与请求代码全等的基金条目并投影为统一报价：[`Quote`]
/// 的公共成员（代码 / 名称 / 价格 / 价格日期）+ 场外通道成员（基金分类 /
/// 净值日期）；市场与类型提示是场内通道成员，基金侧恒缺省（`None`）。
/// `FundBaseInfo` 存在且 FCODE == code 即命中（同码股票 / 指数条目无
/// FundBaseInfo，天然排除；名称凑巧含代码的其他基金被 FCODE 全等判定排除）。
/// 无命中返回 None（查无此码）。
pub(crate) fn pick_fund_quote(resp: &FundSearchResponse, code: &str) -> Option<Quote> {
    let item = resp.datas.as_ref()?.iter().find(|item| {
        item.fund_base_info
            .as_ref()
            .is_some_and(|base| base.fcode == code)
    })?;
    let base = item.fund_base_info.as_ref()?;
    let name = base
        .shortname
        .clone()
        .filter(|n| !n.trim().is_empty())
        .or_else(|| item.name.clone())?;
    // 净值对（值 + 日期）齐备才有效：任一缺省按「未取到净值」处理（不落现价）。
    // 价格在此换算为万分之一元刻度（ADR-0038），与场内通道同载荷同刻度。
    let nav_date = match (base.dwjz, base.fsrq.as_deref()) {
        (Some(nav), Some(date)) if nav > 0.0 && !date.trim().is_empty() => {
            Some((price_value_to_cents(nav), date.trim().to_string()))
        }
        _ => None,
    };
    Some(Quote {
        code: base.fcode.clone(),
        name: name.trim().to_string(),
        price_cents: nav_date.as_ref().map(|(cents, _)| *cents),
        // 场外基金的价格日期即净值日期（现价的行情日期就是净值本身对应的日期）。
        price_date: nav_date.as_ref().map(|(_, date)| date.clone()),
        market: None,
        kind_hint: None,
        fund_class: Some(base.ftype.trim().to_string()),
        nav_date: nav_date.map(|(_, date)| date),
    })
}

/// 按 6 位代码拉取基金报价（名称 / 分类 / 最新净值 + 净值日期）。
/// 搜索通道未命中时回退档案通道（ADR-0039 修订，issue #1212）：已终止（清盘）
/// 基金被东财摘出在用索引、档案文件仍在服务，据此改判为「存在」并回填权威名称
/// 与最后一期单位净值（档案通道无基金分类，该成员缺省）。
/// 两段皆未命中才返回查无此码的中文错误（Invalid），网络失败 / 风控拦截由 HTTP
/// 层重试后上抛。
pub(super) fn fetch_fund_quote(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    code: &str,
) -> Result<Quote> {
    fetch_fund_quote_from(client, pacer, code, FUND_SEARCH_HOSTS, PINGZHONG_HOSTS)
}

/// 同 [`fetch_fund_quote`]，两个通道的主机池都可注入（本地 HTTP 服务测试回退路径）。
pub(super) fn fetch_fund_quote_from(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    code: &str,
    search_hosts: &[&str],
    archive_hosts: &[&str],
) -> Result<Quote> {
    tracing::debug!(code, "基金行情查询");
    let params = [("m", "1"), ("key", code)];
    let resp: FundSearchResponse = request_json_from_hosts(
        client,
        &params,
        FUND_SEARCH_PATH,
        search_hosts,
        RetryConfig::production(),
        pacer,
        &format!("fetch_fund_quote:{code}"),
        None,
    )?;
    if let Some(quote) = pick_fund_quote(&resp, code) {
        return Ok(quote);
    }
    match fetch_fund_archive_from(client, pacer, code, archive_hosts)? {
        Some(archive) => Ok(quote_from_archive(archive, code)),
        None => Err(fund_not_found(code)),
    }
}

/// 档案通道报价投影：公共成员（代码 / 名称 / 价格 / 价格日期）齐备，净值日期同为
/// 价格日期（场外基金现价的行情日期就是净值本身对应的日期）；基金分类是搜索通道
/// 成员，档案通道缺省。
fn quote_from_archive(archive: FundArchive, code: &str) -> Quote {
    let last_nav = archive.last_nav;
    Quote {
        code: code.to_string(),
        name: archive.name,
        price_cents: last_nav
            .as_ref()
            .map(|point| price_value_to_cents(point.nav)),
        price_date: last_nav.as_ref().map(|point| point.date.clone()),
        market: None,
        kind_hint: None,
        fund_class: None,
        nav_date: last_nav.map(|point| point.date),
    }
}

/// 「查无此码」码化错误（Invalid → 400）：搜索与档案两段皆未命中时的唯一出口。
fn fund_not_found(code: &str) -> AppError {
    AppError::codedp(
        "sync.fund-not-found",
        format!("查无基金代码 {code}，请核对后重试"),
        &[code],
    )
}

/// 生产拉取入口：构建客户端与限流器后执行单次详情查询（不经数据库连接，
/// 供 IPC 命令在获取连接锁之前完成网络往返，避免长限流重试阻塞其它命令）。
pub fn fetch_fund_quote_production(code: &str) -> Result<Quote> {
    let client = build_client()?;
    let mut pacer = Pacer::default();
    fetch_fund_quote(&client, &mut pacer, code)
}
