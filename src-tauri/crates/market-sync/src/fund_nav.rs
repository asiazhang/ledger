//! 东财历史净值通道（issue #303 / ADR-0038 决策 6）：lsjz 历史净值接口访问、
//! 报文解析、净值同步水位语义与基金分区编排；首刷深回填另走单请求全量通道
//!（基金详情页数据文件，issue #1062）。
//!
//! 货币基金口径（issue #1342）：货基的万份收益列不是单位净值（货基单位净值恒
//! [`MONEY_FUND_UNIT_NAV`]，收益以份额结转体现），三个取数面各自响应自报口径
//! 判定——lsjz 看 `SYType`/`FundType`，搜索通道看 `FUNDTYPE`（[`is_money_fund_type_code`]），
//! 详情页数据文件看「缺单位净值序列而有万份收益序列」；命中即归一化为恒定净值，
//! 万份收益数值与累计净值都不参与（份额已含结转收益，再乘累计净值会重复计）。
//!
//! - 报文解析（[`parse_lsjz`]）与水位窗口（[`nav_window`]）为纯函数，fixture
//!   单测见 `tests/fund_nav.rs`（真实报文形状，不依赖真实网络）；
//! - 页抓取（[`fetch_nav_page`]）复用行情 HTTP 层的主机池 / 重试 / 限流泛型层；
//!   lsjz 为单主机接口且必须携带 Referer 头（缺省被以 ErrCode=-999 拦截）；
//! - 单请求全量通道（[`fetch_nav_full_series`]）一次 GET 基金详情页数据文件、解析
//!   `Data_netWorthTrend` 得整只基金历史单位净值（口径与 lsjz `DWJZ` 逐值一致，
//!   ADR-0038 修订记录有样本验证），仅首刷深回填用——抓取 / 解析失败或窗口内无点
//!   fail-closed 回退 lsjz 分页，不静默丢数据；
//! - 逐只编排分两单元（issue #1377 现价与历史解耦收口）：现价刷新
//!   [`refresh_one_fund_price`]（服务标的信息同步，只刷现价——取数面命中整只零
//!   请求，未命中退逐只短窗、页数封顶）；历史回填 [`backfill_one_fund_history`]
//!   （服务价格历史后台补全）：首刷判据 = **磁盘上没有任何历史序列**（issue #1059
//!   ——添加基金 / AI 导入已把最新净值落现价缓存、水位有值但 PriceHistory 为空，
//!   此时按首刷回填近两年）；已有历史序列的基金以现价缓存的净值日期
//!   （`market_prices.nav_date`，#301 落）为水位，从水位次日起按页增量（常态每只一页，
//!   页大小为服务端硬上限 20）；全部净值点攒齐后一次降采样落周线（跨页同周取最后
//!   一个净值日），现价 = 窗口内最新公布单位净值。基金间的遍历与名称随行刷新、标的级
//!   进度推进归增量同步编排（`incremental`，issue #897 逐只合并推进）；每页抓取返回
//!   后的页级推进经注入回调透传（issue #1061）。

use chrono::NaiveDate;
use rusqlite::params;
use serde::Deserialize;

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, price_value_to_cents, upsert_market_price,
    upsert_price_history,
};

use super::bulk::BulkNavPoint;
use super::fund::{deserialize_flexible_f64, deserialize_flexible_string};
use super::http::{KlineBar, Pacer, RetryConfig, request_json_from_hosts, request_text_from_hosts};
use super::session::ScopedSession;

// 历史净值接口：单主机（无公开镜像池），复用行情层的重试与限流泛型层。
const LSJZ_HOSTS: &[&str] = &["https://api.fund.eastmoney.com"];
const LSJZ_PATH: &str = "/f10/lsjz";
/// 每页条数：服务端硬上限（请求更大值实测仍按 20 生效，2026-08），分页循环按此定界。
const LSJZ_PAGE_SIZE: u64 = 20;
/// 单只基金单次同步的页数上限：近两年窗口约 25 页（≈500 个净值日 ÷ 20），
/// 上限兜底防异常 TotalCount 导致的失控翻页。触顶即窗口已知未采全，整只不落库
///（见 [`NavPages::truncated`]，ADR-0122 决策 8 / issue #1373）。
const MAX_NAV_PAGES: u64 = 40;

/// 现价刷新逐只回退的页数封顶（issue #1377）：现价刷新不是历史采集，逐只回退只
/// 服务「把现价刷到最新」与顺带落上近几个缺周点；封顶 2 页（≈ 8 周净值日）内
/// 窗口完整才落库，更深的缺口整只不落、交价格历史后台补全在下一窗口深采——
/// 水位不得推进到未落周点之前（否则缺周点永久漏采，见 [`refresh_one_fund_price`]）。
const REFRESH_MAX_NAV_PAGES: u64 = 2;

/// 现价刷新逐只回退的无历史序列短窗（issue #1377）：首刷的历史由后台补全整根
/// 回填，现价刷新只为这类标的拿「最新公布净值」——一个月窗口单页量级、常数请求。
const REFRESH_RECENT_WINDOW_MONTHS: chrono::Months = chrono::Months::new(1);

// 基金详情页数据文件（pingzhongdata）：单主机（无公开镜像池），一次请求返回整只
// 基金的全部历史净值（issue #1062）。仅服务首刷深回填，增量仍走 lsjz。
pub(super) const PINGZHONG_HOSTS: &[&str] = &["https://fund.eastmoney.com"];
const PINGZHONG_PATH_PREFIX: &str = "/pingzhongdata/";

/// lsjz 整体响应。`TotalCount` 在顶层；`Data` 正常为对象，被拦截形态（缺
/// Referer / 风控）是空字符串，以无标签枚举宽容为 [`LsjzDataField::Blocked`]；
/// 缺省（Data 字段不存在）为 None。
#[derive(Debug, Deserialize)]
pub(super) struct LsjzResponse {
    #[serde(rename = "Data", default)]
    pub(super) data: Option<LsjzDataField>,
    #[serde(rename = "TotalCount", default)]
    pub(super) total_count: u64,
}

/// `Data` 字段的两种 wire 形态（见 [`LsjzResponse`]）。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum LsjzDataField {
    Data(LsjzData),
    /// 被拦截形态兜底（`Data:""` 等）：payload 仅为宽容解析的承接容器，
    /// 解析层不消费其内容（整体按空处理并记 debug 日志）。
    #[allow(dead_code)]
    Blocked(serde_json::Value),
}

#[derive(Debug, Deserialize)]
pub(super) struct LsjzData {
    #[serde(rename = "LSJZList", default)]
    pub(super) lsjz_list: Option<Vec<LsjzItem>>,
    /// 东财基金类型码（`005` = 货币型，与搜索通道 `FUNDTYPE` 同一枚代码表，
    /// issue #1342）；宽容解析，未知形态归缺省（不使整页报文失败），已终止
    /// 基金等形态会缺省。
    #[serde(
        rename = "FundType",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    pub(super) fund_type: Option<String>,
    /// 收益披露口径声明：`每万份收益` = `DWJZ` 列承载的是万份收益而非单位
    /// 净值（issue #1342）；非收益型基金缺省。宽容解析同上。
    #[serde(
        rename = "SYType",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    pub(super) sy_type: Option<String>,
}

/// 历史净值单行：净值日期 + 单位净值。只解析消费的两列，其余（累计净值、
/// 申购赎回状态等）忽略。
#[derive(Debug, Deserialize)]
pub(super) struct LsjzItem {
    /// 净值日期（ISO）。
    #[serde(rename = "FSRQ")]
    pub(super) fsrq: String,
    /// 单位净值（真实价格值，元）：数字或数字字符串；未公布为 null / 空串。
    #[serde(
        rename = "DWJZ",
        default,
        deserialize_with = "deserialize_flexible_f64"
    )]
    pub(super) dwjz: Option<f64>,
}

/// 一个净值采样点：净值日期 + 单位净值（真实价格值，元）。
#[derive(Debug, Clone, PartialEq)]
pub struct NavPoint {
    pub(super) date: String,
    pub(super) nav: f64,
}

/// 基金详情页数据文件（pingzhongdata）里单位净值序列的变量名。同文件另有
/// `Data_ACWorthTrend`（累计净值，`[时间戳, 值]` 数组）——本通道刻意只取单位
/// 净值（与历史净值接口 `DWJZ` 同口径，见 [`parse_net_worth_trend`]）。
const NET_WORTH_TREND_VAR: &str = "Data_netWorthTrend";

/// 同一数据文件里万份收益序列的变量名（`[北京时间午夜毫秒时间戳, 万份收益]`
/// 升序数组）：货币基金没有单位净值序列，本通道以「缺 [`NET_WORTH_TREND_VAR`]
/// 而有本序列」为货基特征，收益日期 × 恒定单位净值收录（issue #1342）。
const MONEY_INCOME_TREND_VAR: &str = "Data_millionCopiesIncome";

/// 东财基金类型码「货币型」：搜索通道 `FUNDTYPE` 与历史净值接口 `FundType`
/// 共用同一枚代码表（issue #1342）。
const MONEY_FUND_TYPE_CODE: &str = "005";

/// 历史净值接口 `SYType` 的收益披露声明值：`DWJZ` 列承载万份收益而非单位净值
///（issue #1342）。
const MONEY_FUND_INCOME_SYTYPE: &str = "每万份收益";

/// 货币基金的恒定单位净值（issue #1342）：收益以份额结转体现，单位净值恒为
/// 1.0000——现价与净值序列都按此值收录，万份收益数值不参与。
pub(super) const MONEY_FUND_UNIT_NAV: f64 = 1.0;

/// 按东财基金类型码判定货币基金（issue #1342）：搜索通道（`FUNDTYPE`）与
/// 历史净值接口（`FundType`）共用，代码表 [`MONEY_FUND_TYPE_CODE`]。
pub(super) fn is_money_fund_type_code(code: &str) -> bool {
    code.trim() == MONEY_FUND_TYPE_CODE
}

/// 历史净值接口响应的货币基金判定（issue #1342）：接口自报「收益口径 = 每万
/// 份收益」（`DWJZ` 列不是净值）或基金类型码为货币型，任一命中即按货币基金
/// 口径收录。两个信号实测同现；任一缺省（已终止基金等形态）由另一个兜住。
fn is_money_fund_lsjz(sy_type: Option<&str>, fund_type: Option<&str>) -> bool {
    sy_type.map(str::trim) == Some(MONEY_FUND_INCOME_SYTYPE)
        || fund_type.is_some_and(is_money_fund_type_code)
}

/// 同一数据文件里的基金名称与代码变量名（issue #1212 / ADR-0039 修订）：档案通道
/// 据此判定「这份文件是不是本基金的」并取权威名称。
const FUND_NAME_VAR: &str = "fS_name";
const FUND_CODE_VAR: &str = "fS_code";

/// 单位净值序列的单个元素：`x` = 净值日北京时间午夜的毫秒时间戳；`y` = 单位
/// 净值（真实价格值，元）；其余字段（equityReturn / unitMoney）不消费。
#[derive(Debug, Deserialize)]
struct NetWorthTrendPoint {
    x: i64,
    #[serde(default, deserialize_with = "deserialize_flexible_f64")]
    y: Option<f64>,
}

/// 解析基金详情页数据文件（`.js`）里的**单位净值序列**（issue #1062）：取出
/// `Data_netWorthTrend` 数组并投影为 [`NavPoint`]（毫秒时间戳 + 8h 取北京日历日
/// 即净值日期），无效行（缺净值 / 净值 ≤ 0 / 时间戳越界）静默过滤，与 lsjz 同姿态。
///
/// 返回值区分两种语义：`None` = 变量缺省 / 数组截断 / 字段形态不符——数据不可信，
/// 调用方 fail-closed 回退分页通道；`Some(vec![])` = 结构完好但序列为空（新基金
/// 未公布净值），是可信空结果。累计净值数组 `Data_ACWorthTrend` 不被消费。
pub(super) fn parse_net_worth_trend(js: &str) -> Option<Vec<NavPoint>> {
    let array = super::js::declared_array(js, NET_WORTH_TREND_VAR)?;
    let raw: Vec<NetWorthTrendPoint> = serde_json::from_str(array).ok()?;
    Some(
        raw.into_iter()
            .filter_map(|point| {
                let nav = point.y.filter(|n| *n > 0.0)?;
                let date = beijing_date_from_epoch_ms(point.x)?;
                Some(NavPoint { date, nav })
            })
            .collect(),
    )
}

/// 档案通道（基金详情页数据文件）的基金档案：权威名称 + 最后一期单位净值。搜索
/// 索引只收在用基金，已终止（清盘）基金被东财摘出该索引（ADR-0039 修订，issue
/// #1212），这两项由档案通道承接。
pub(super) struct FundArchive {
    pub(super) name: String,
    pub(super) last_nav: Option<NavPoint>,
}

/// 序列里的最新净值点（按净值日期；水位与「最后一期净值」同此判）。
fn latest_point(points: Option<Vec<NavPoint>>) -> Option<NavPoint> {
    points?.into_iter().max_by(|a, b| a.date.cmp(&b.date))
}

/// 解析档案通道响应：`fS_code` 与请求代码全等且 `fS_name` 非空才命中（与搜索通道
/// 的 FCODE 全等同一防御纪律）；未声明 / 形态不符 / 代码不符均返回 None，由调用方
/// 按「查无此码」处置。单位净值序列缺失或不可信时按「未取到净值」降级（名称仍可用），
/// 与搜索通道「命中但未公布净值」同形。
pub(super) fn parse_fund_archive(js: &str, code: &str) -> Option<FundArchive> {
    let declared_code = super::js::declared_string(js, FUND_CODE_VAR)?;
    if declared_code != code {
        return None;
    }
    let name = super::js::declared_string(js, FUND_NAME_VAR)?.trim();
    if name.is_empty() {
        return None;
    }
    let last_nav = latest_point(parse_net_worth_trend(js)).or_else(|| {
        // 货基没有单位净值序列：最后一期净值 = 最新收益日 × 恒定单位净值
        // 1.0000（issue #1342）——档案回退报价据此落 1.0000 而非无价。
        latest_point(parse_money_fund_income_series(js))
    });
    Some(FundArchive {
        name: name.to_string(),
        last_nav,
    })
}

/// 毫秒时间戳（净值日北京时间午夜，见单位净值序列元素的 `x`）→ ISO 净值日期：
/// 委托北京日历日单点 [`super::incremental::beijing_date`]（UTC + 8h 取日期部分），
/// 不在此复刻 +8h 口径。
fn beijing_date_from_epoch_ms(ms: i64) -> Option<String> {
    let utc = chrono::DateTime::from_timestamp_millis(ms)?;
    Some(
        super::incremental::beijing_date(utc)
            .format("%Y-%m-%d")
            .to_string(),
    )
}

/// 解析货币基金档案文件里的**万份收益序列**（issue #1342）：货基没有单位净值
/// 序列（`Data_netWorthTrend` 缺省），单位净值恒 [`MONEY_FUND_UNIT_NAV`]，本
/// 函数把收益序列的日期投影成净值点（收益值不消费，含 0 与偶发负值）；时间戳
/// 不可解析的行静默过滤，与 [`parse_net_worth_trend`] 同姿态。
///
/// 返回值语义与 [`parse_net_worth_trend`] 一致：`None` = 变量缺省 / 数组截断 /
/// 元素形态不符——数据不可信，调用方 fail-closed 回退分页通道；`Some(vec![])`
/// = 结构完好但序列为空（新基金无收益记录），是可信空结果。
pub(super) fn parse_money_fund_income_series(js: &str) -> Option<Vec<NavPoint>> {
    let array = super::js::declared_array(js, MONEY_INCOME_TREND_VAR)?;
    // 元素为 `[毫秒时间戳, 万份收益]` 对：时间戳即净值日本体；收益值类型放宽
    // 承接任意形态（不消费），只为保住日期。
    let raw: Vec<(i64, serde_json::Value)> = serde_json::from_str(array).ok()?;
    Some(
        raw.into_iter()
            .filter_map(|(timestamp, _)| {
                let date = beijing_date_from_epoch_ms(timestamp)?;
                Some(NavPoint {
                    date,
                    nav: MONEY_FUND_UNIT_NAV,
                })
            })
            .collect(),
    )
}

/// 一页净值的解析结果：有效净值点 + 窗口内总条数（服务端按起止日期过滤后的
/// 总数，供分页循环定界）+ 报文形态（`blocked` = 空响应/被拦截，见
/// [`parse_lsjz`]）。
#[derive(Debug, Clone, PartialEq)]
pub struct LsjzPage {
    pub(super) points: Vec<NavPoint>,
    pub(super) total: u64,
    /// 空响应/异常形态（`Data` 缺省或非对象，如缺 Referer 被拦截 / 风控）：
    /// 空结果不可信，不得按「窗口内确实无新净值」计成功（issue #1059）。
    pub(super) blocked: bool,
}

/// 一只基金的单页查询（注入接缝的请求形状）：日期闭区间、页码 1 起。
#[derive(Debug, Clone, PartialEq)]
pub struct NavQuery {
    pub(super) code: String,
    pub(super) start_date: String,
    pub(super) end_date: String,
    pub(super) page: u64,
}

/// 解析一页 lsjz 报文：挑出有效净值点（日期非空、单位净值 > 0；未公布/异常行
/// 静默过滤，与日线「无效样本不中断」同一姿态）+ 顶层总条数 + 报文形态。被拦截
/// 形态（`Data:""` / [`LsjzDataField::Blocked`]，含 `Data` 缺省）得空表并标记
/// `blocked`——空表有两种语义（抓取不可信 vs 窗口内确实没有新净值），解析层
/// 负责把它们区分开（issue #1059）。
///
/// 货币基金（[`is_money_fund_lsjz`] 命中，issue #1342）：`DWJZ` 列是万份收益
/// 而非单位净值，单位净值恒 [`MONEY_FUND_UNIT_NAV`]——日期即净值日本体，
/// 收益值是否在场 / 为何值（含 0 与偶发负值）不影响行有效性，不进价格。
pub(super) fn parse_lsjz(resp: &LsjzResponse) -> LsjzPage {
    let data = match &resp.data {
        Some(LsjzDataField::Data(data)) => data,
        other => {
            tracing::debug!(payload = ?other, "lsjz Data 缺省或为被拦截形态，按空响应处理");
            return LsjzPage {
                points: Vec::new(),
                total: resp.total_count,
                blocked: true,
            };
        }
    };
    let money_fund = is_money_fund_lsjz(data.sy_type.as_deref(), data.fund_type.as_deref());
    let points = data
        .lsjz_list
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|item| {
            let date = item.fsrq.trim();
            if date.is_empty() {
                return None;
            }
            let nav = if money_fund {
                MONEY_FUND_UNIT_NAV
            } else {
                item.dwjz.filter(|nav| *nav > 0.0)?
            };
            Some(NavPoint {
                date: date.to_string(),
                nav,
            })
        })
        .collect();
    LsjzPage {
        points,
        total: resp.total_count,
        blocked: false,
    }
}

/// 净值同步窗口（ADR-0038 决策 6）：有水位（现价缓存的净值日期）则从水位
/// 次日起（增量）；无水位为首刷，回填近两年。水位非法（理论不可达——写入侧
/// 恒为接口返回的 ISO 日期）按首刷兜底自愈，重复回填由周采样幂等覆盖吸收。
/// 起点只由入参水位决定；「是否首刷」由调用方（[`backfill_one_fund_history`]）按有无
/// 历史序列判定后传 None 表达（issue #1059），本函数不含那一层判据。
/// 返回 `(start, end)` 闭区间（ISO 日期）。
pub(super) fn nav_window(watermark: Option<&str>, today: NaiveDate) -> (String, String) {
    let start = match watermark.map(str::trim) {
        Some(w) if !w.is_empty() => match NaiveDate::parse_from_str(w, "%Y-%m-%d") {
            Ok(d) => d.succ_opt().unwrap_or(d),
            Err(_) => {
                tracing::warn!(watermark = %w, "净值水位非法，按首刷回填近两年");
                super::incremental::two_years_ago(today)
            }
        },
        _ => super::incremental::two_years_ago(today),
    };
    (
        start.format("%Y-%m-%d").to_string(),
        today.format("%Y-%m-%d").to_string(),
    )
}

/// lsjz 请求的 Referer（模拟官站 f10 历史净值页跳来源；缺 Referer 会被接口
/// 以 ErrCode=-999 拦截，见模块文档）。
fn nav_referer(code: &str) -> String {
    format!("http://fundf10.eastmoney.com/jjjz_{code}.html")
}

/// 拉取一只基金的一页历史净值（生产主机池）。窗口由 `query` 闭区间给定。
pub(super) fn fetch_nav_page(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    query: &NavQuery,
) -> Result<LsjzPage> {
    fetch_nav_page_from(client, pacer, query, LSJZ_HOSTS)
}

/// 同 [`fetch_nav_page`]，主机池可注入（本地 HTTP 服务测试 Referer 传播）。
pub(super) fn fetch_nav_page_from(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    query: &NavQuery,
    hosts: &[&str],
) -> Result<LsjzPage> {
    tracing::debug!(
        code = %query.code, page = %query.page,
        start = %query.start_date, end = %query.end_date,
        "历史净值页查询"
    );
    let page_str = query.page.to_string();
    let size_str = LSJZ_PAGE_SIZE.to_string();
    let params = [
        ("fundCode", query.code.as_str()),
        ("pageIndex", page_str.as_str()),
        ("pageSize", size_str.as_str()),
        ("startDate", query.start_date.as_str()),
        ("endDate", query.end_date.as_str()),
    ];
    let referer = nav_referer(&query.code);
    let resp: LsjzResponse = request_json_from_hosts(
        client,
        &params,
        LSJZ_PATH,
        hosts,
        RetryConfig::production(),
        pacer,
        &format!("fetch_nav_page:{}", query.code),
        Some(referer.as_str()),
    )?;
    Ok(parse_lsjz(&resp))
}

/// 单请求全量净值通道（issue #1062）：一次 GET 基金详情页数据文件，解析
/// 档案通道取数（issue #1212 / ADR-0039 修订）：一次 GET 基金详情页数据文件，解析
/// 出权威名称与最后一期单位净值；主机池可注入（本地 HTTP 服务测试请求路径与解析）。
/// `Ok(None)` = 这份文件不是本基金的（含无效代码被重定向到错误页的形态），由调用方
/// 按查无此码处置；`Err` 只留给传输类失败（网络 / 限流耗尽重试），与既有「网络失败
/// 上抛」契约一致。
pub(super) fn fetch_fund_archive_from(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    code: &str,
    hosts: &[&str],
) -> Result<Option<FundArchive>> {
    tracing::debug!(code, "基金档案通道查询");
    let path = format!("{PINGZHONG_PATH_PREFIX}{code}.js");
    let body = request_text_from_hosts(
        client,
        &[],
        &path,
        hosts,
        RetryConfig::production(),
        pacer,
        &format!("fetch_fund_archive:{code}"),
        None,
    )?;
    Ok(parse_fund_archive(&body, code))
}

/// 单请求全量净值通道（issue #1062）：一次 GET 基金详情页数据文件，解析
/// `Data_netWorthTrend` 得整只基金的**全部历史单位净值**——口径与 lsjz 的 `DWJZ`
/// 逐值一致、同落在万分位价格刻度上（spike 四类型样本验证见 ADR-0038 修订记录）。
/// 仅服务首刷深回填，替代约 25 次分页请求；日常增量仍走 lsjz。
///
/// 失败语义是 fail-closed 的前半：网络失败、被拦截（HTML 而非数据文件）或解析不出
/// 单位净值序列都返回 `Err`，调用方据此回退分页通道，不把不可信结果当「无净值」。
pub(super) fn fetch_nav_full_series(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    code: &str,
) -> Result<Vec<NavPoint>> {
    fetch_nav_full_series_from(client, pacer, code, PINGZHONG_HOSTS)
}

/// 同 [`fetch_nav_full_series`]，主机池可注入（本地 HTTP 服务测试请求路径与解析）。
pub(super) fn fetch_nav_full_series_from(
    client: &reqwest::blocking::Client,
    pacer: &mut Pacer,
    code: &str,
    hosts: &[&str],
) -> Result<Vec<NavPoint>> {
    tracing::debug!(code, "单请求全量净值查询");
    let path = format!("{PINGZHONG_PATH_PREFIX}{code}.js");
    let body = request_text_from_hosts(
        client,
        &[],
        &path,
        hosts,
        RetryConfig::production(),
        pacer,
        &format!("fetch_nav_full_series:{code}"),
        None,
    )?;
    // 先按单位净值序列解析；货币基金没有该序列，按万份收益序列收录（日期 ×
    // 恒定单位净值 1.0000，issue #1342）。两段皆不可信才 Err——调用方据此
    // fail-closed 回退分页通道，不把不可信结果当「无净值」。
    parse_net_worth_trend(&body)
        .or_else(|| parse_money_fund_income_series(&body))
        .ok_or_else(|| {
            // 文本通道的解析恒成功，疑似风控页（HTML 而非数据文件）在 HTTP 层看不见
            // ——降速信号由做可信度判定的这一层补上（ADR-0121 决策 5）。
            pacer.record_throttled();
            AppError::Parse(format!("基金 {code} 详情页数据文件缺少可信的净值序列"))
        })
}

/// 基金分区的同步统计（与 [`super::incremental`] 的股票统计同源汇总）：
/// `synced` = 处理成功（含「已是最新、无新净值」）；`skipped` = 无法拉取
/// （首刷查无净值；**空响应/被拦截**——不是「无新净值」，issue #1059；名称充
/// 代码行的跳过计数由编排层派生，不入本结构）；`written` = 实际落库净值的只数
///（价格失效信号判定依据，零变化不广播）。
pub(super) struct FundSyncStats {
    pub(super) synced: usize,
    pub(super) skipped: usize,
    pub(super) written: usize,
}

/// 基金分区水位与首刷判据的共享读（issue #1377 自两单元抽出）：水位 = 现价缓存
/// 的净值日期；首刷判据 = 磁盘上没有任何历史序列（issue #1059）。两条读经作用域
/// 会话短暂取一次连接完成（issue #1275）；抓取前不再触碰连接。
fn read_fund_watermark<Q: ScopedSession>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
) -> Result<(Option<String>, bool)> {
    session.with_connection(|conn| {
        let watermark: Option<String> = conn
            .query_row(
                "SELECT nav_date FROM market_prices WHERE instrument_id=?1",
                params![fund.instrument_id],
                |r| r.get(0),
            )
            .ok();
        let has_history = ledger_investment::backfill::has_any_history(conn, &fund.instrument_id)?;
        Ok((watermark, has_history))
    })
}

/// 单只 fund 标的的现价刷新单元（ADR-0122 决策 2/3 / issue #1377）：只刷现价、
/// 不承担历史——首刷深回填与缺周点深补归价格历史后台补全
///（[`backfill_one_fund_history`]）。取数面（[`super::bulk`]）命中时整只零请求：
/// 无新净值（批量面最新净值日期不晚于水位）即「已是最新」；新净值直落现价缓存，
/// 有历史序列者连当周采样点一并落库（[`land_bulk_point`]，周采样语义不变）。
/// 未命中（缺口 / 降级 / 停用期）退回逐只通道，窗口按「现价刷新」收窄——
/// 有历史序列者从水位次日增量但页数封顶（[`REFRESH_MAX_NAV_PAGES`]）：窗口
/// 完整才整只落库；更深缺口整只不落、交后台补全——**水位不得推进到未落周点
/// 之前**，否则缺周点在队列判定（水位判缺）里永久消失。无历史序列者取近一个月
/// 短窗（[`REFRESH_RECENT_WINDOW_MONTHS`]）的最新净值落现价，历史一行为零、
/// 不落采样点（历史归后台补全首刷）。
/// 单只结果累加进调用方的 `stats`；页级推进经注入的 `on_page` 回调透传
///（issue #1061）。基金间的遍历、名称随行刷新与标的级进度推进归编排层
///（issue #897）。跳过语义与 [`backfill_one_fund_history`] 一致：查无净值与
/// 空响应/被拦截计入 `skipped`，不报错不中断；单只网络失败上抛中断同步。
///
/// **前置条件**：`fund` 为 6 位真实代码的有通道基金行——名称充代码行（查不到
/// 净值）由调用方计入跳过、零请求（issue #897 起跳过判定与分母口径同收编排层）。
pub(super) fn refresh_one_fund_price<Q, N, P>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
    latest_hint: Option<&BulkNavPoint>,
    fetch_nav: &mut N,
    stats: &mut FundSyncStats,
    on_page: &mut P,
) -> Result<()>
where
    // 作用域会话接缝（issue #1275）：本函数读写库的唯一通道，签名层面取不到连接。
    Q: ScopedSession,
    N: FnMut(&NavQuery) -> Result<LsjzPage>,
    // 页级推进回调（issue #1061）：(已完成页, 总页数)。只在本页抓取返回之后发出
    //（抓取内部的退避/重试等待不产生推进）；单页（pages ≤ 1）不发——增量常态
    // 的事件形状与频率不变。
    P: FnMut(u64, u64),
{
    let today = super::incremental::beijing_today();
    let (watermark, has_history) = read_fund_watermark(session, fund)?;
    // 取数面命中时整只零请求（ADR-0121 取数面把「要不要发逐只请求」的判断从
    // 「每标的一次请求」降为「整市场一次请求」）——逐只通道只在缺周点补齐与
    // 批量面未覆盖时接管（issue #1377：首刷不再接管，历史归后台补全）。
    match bulk_decision(latest_hint, watermark.as_deref()) {
        BulkDecision::NothingNew => {
            stats.synced += 1;
            return Ok(());
        }
        BulkDecision::LandFromBulk(hint) => {
            land_bulk_point(session, fund, hint)?;
            stats.synced += 1;
            stats.written += 1;
            return Ok(());
        }
        BulkDecision::PerInstrument => {
            if let Some(hint) = latest_hint {
                tracing::debug!(
                    code = %fund.symbol, watermark = ?watermark, bulk_date = %hint.date,
                    "批量面已报出新净值但水位落后超过一个自然周，逐只短窗补齐缺失周点（封顶页数内）"
                );
            }
        }
    }
    // 逐只回退的窗口按「现价刷新」收窄（issue #1377）：有历史序列者从水位次日
    // 增量（封顶页数内窗口完整才落库）；无历史序列者取近一个月短窗拿最新净值。
    let (start, end) = if has_history {
        nav_window(watermark.as_deref(), today)
    } else {
        let start = today
            .checked_sub_months(REFRESH_RECENT_WINDOW_MONTHS)
            .unwrap_or(today);
        (
            start.format("%Y-%m-%d").to_string(),
            today.format("%Y-%m-%d").to_string(),
        )
    };
    let collected = fetch_nav_pages(
        fetch_nav,
        &fund.symbol,
        &start,
        &end,
        REFRESH_MAX_NAV_PAGES,
        on_page,
    )?;

    if collected.points.is_empty() {
        if collected.blocked {
            // 空响应（Data 缺省 / 非对象）= 疑似被拦截或异常：结果不可信，不
            // 得计入成功（issue #1059）。与「窗口内确实无新净值」在统计上分
            // 开——此路计入跳过；日志带出原形态，便于与 T+1 正常空窗对照。
            tracing::warn!(
                code = %fund.symbol, has_history, watermark = ?watermark,
                "历史净值返回空响应（疑似被拦截/异常），本轮无法判定是否有新净值"
            );
            stats.skipped += 1;
        } else if has_history {
            // 增量窗口内确实无新净值（T+1 正常空窗）：现价已是最新，处理成功
            // 但不落库、不计跳过。
            stats.synced += 1;
        } else {
            // 查无净值（查无此码 / 新基金未公布首期）：无法拉取，计入跳过。
            stats.skipped += 1;
        }
        return Ok(());
    }
    if !collected.is_complete() {
        // 窗口不完整（深度缺周点超出封顶页数或页被拦截，issue #1377）：整只
        // 不落库并在日志标注——水位不得推进到未落周点之前（否则缺周点永久
        // 漏采），交价格历史后台补全在下一窗口深采；现价刷新不承担深补。
        tracing::warn!(
            code = %fund.symbol, has_history,
            blocked = collected.blocked, truncated = collected.truncated,
            "现价刷新短窗不完整（深度缺周点或被拦截），整只不落库、交后台补全深采"
        );
        stats.skipped += 1;
        return Ok(());
    }
    let points = collected.points;
    // 现价 = 窗口内最新公布单位净值（let-else 显式防线，#434 同款）：points
    // 非空由前文判空保证，此臂理论不可达；一旦前置防线被移除，此处记警告并
    // 跳过该只、不中断同步。
    let Some(latest) = points.iter().max_by_key(|p| p.date.as_str()) else {
        tracing::warn!(code = %fund.symbol, "净值点意外为空，跳过现价更新");
        return Ok(());
    };
    // 无新净值防线：短窗拿到的最新净值不新于水位即「已是最新」（无历史序列者
    // 的常态——按代码即拉已落过同一净值），零写入不虚增价格失效信号。
    if watermark
        .as_deref()
        .is_some_and(|w| latest.date.as_str() <= w)
    {
        stats.synced += 1;
        return Ok(());
    }
    // 当周采样点（仅有历史序列者）与现价落库同在一只一个事务里（ADR-0122
    // 决策 8 / issue #1373 同形体）：写失败整体回滚，不留半根。
    let bars: Vec<KlineBar> = points
        .iter()
        .map(|p| KlineBar {
            date: p.date.clone(),
            close: p.nav,
        })
        .collect();
    session.with_connection(|conn| {
        ensure_transaction(conn, || {
            if has_history {
                // 窗口内缺失的近期周点顺带落库（封顶页数保证了窗口完整；
                // 深度缺周点已在上方交后台补全）。
                super::incremental::write_weekly_price_history(
                    conn,
                    &fund.instrument_id,
                    &fund.currency,
                    &bars,
                )?;
            }
            upsert_market_price(
                conn,
                &MarketPriceWrite {
                    instrument_id: &fund.instrument_id,
                    price_cents: price_value_to_cents(latest.nav),
                    currency_code: &fund.currency,
                    // 基金现价时点 = 净值日期（现价的行情日期就是净值本身对应的日期）；
                    // nav_date 兼任下次同步的水位。
                    priced_at: &latest.date,
                    nav_date: Some(&latest.date),
                    source: Some(EASTMONEY_PRICE_SOURCE),
                },
            )?;
            Ok(())
        })
    })?;
    stats.synced += 1;
    stats.written += 1;
    Ok(())
}

/// 单只基金历史回填的结局（issue #1377 走势空态三态的判据输入）：`written` =
/// 是否实际落库；`inconclusive` = 本轮窗口不完整（部分页空响应或页数触顶）——
/// 整只不落库、结果不可信，后台补全据此把该只记为「待重试」而非「无数据」
///（被拦截 ≠ 数据源没有可采序列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BackfillOutcome {
    pub(super) written: bool,
    pub(super) inconclusive: bool,
}

/// 单只 fund 标的的**历史回填**单元（ADR-0038 决策 6 / ADR-0122 决策 2；issue
/// #1377 起归价格历史后台补全专用——现价刷新走 [`refresh_one_fund_price`]，
/// 「同步标的信息」不再承担首刷）：**首刷判据 = 磁盘上没有任何历史序列**（issue
/// #1059）。首刷回填近两年——优先走单请求全量通道（详情页数据文件，issue
/// #1062），抓取/解析失败或窗口内无点则 fail-closed 回退 lsjz 分页通道；已有
/// 历史序列的基金走 lsjz 分页，以现价缓存的净值日期为水位增量（水位当日不重拉、
/// 只取水位次日之后）。净值点降采样落 PriceHistory（同周整周覆盖幂等），窗口内
/// 最新公布净值落现价缓存（现价 = 单位净值、priced_at = nav_date = 净值日期，
/// 与 #301 添加基金同形）。周采样与现价落库在**一只一个事务**里整只一次提交
///（ADR-0122 决策 8 / issue #1373）：第 N 个周点写入失败或中途中断整体回滚，
/// 磁盘上不留半根历史——「有历史序列」与「历史完整」由此等价，首刷判据（ADR-0038
/// 决策 6）依赖的正是这个等价。页级推进经注入的 `on_page` 回调透传（issue
/// #1061——每页抓取返回后报告「已完成页/总页数」，抓取内部的退避/重试等待不
/// 产生推进）。
///
/// **前置条件**：`fund` 为 6 位真实代码的有通道基金行。跳过语义：首刷查无净值
/// 与**空响应/被拦截**（issue #1059）不报错不中断，以 [`BackfillOutcome`] 表达
/// 结局；其中**本轮窗口不完整**（部分页空响应或页数触顶，ADR-0122 决策 8 /
/// issue #1373）整只不落库并记 `inconclusive`，不留半根历史；单只网络失败上抛
///（后台补全编排单只失败不中断本轮）。
pub(super) fn backfill_one_fund_history<Q, N, S, P>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
    fetch_nav: &mut N,
    fetch_nav_full_series: &mut S,
    on_page: &mut P,
) -> Result<BackfillOutcome>
where
    // 作用域会话接缝（issue #1275）：本函数读写库的唯一通道，签名层面取不到连接。
    Q: ScopedSession,
    N: FnMut(&NavQuery) -> Result<LsjzPage>,
    S: FnMut(&str) -> Result<Vec<NavPoint>>,
    P: FnMut(u64, u64),
{
    let today = super::incremental::beijing_today();
    // 水位 = 现价缓存的净值日期（股票行恒 NULL，基金行由 #301/本通道写入）；首刷
    // 判据 = 磁盘上没有任何历史序列（issue #1059）：添加基金 / AI 导入在
    // 「按代码即拉」时已把最新净值落现价缓存（水位有值）但 PriceHistory 为空，
    // 若以水位作增量起点，增量窗口只剩「水位次日」而近两年回填静默落空
    //（#303 验收在真实账本上未成立的根因）。水位只服务已有历史序列的增量。
    // 两条读经作用域会话短暂取一次连接完成（issue #1275）；抓取前不再触碰连接。
    let (watermark, has_history) = read_fund_watermark(session, fund)?;
    let first_fill = !has_history;
    let window_watermark = if first_fill {
        None
    } else {
        watermark.as_deref()
    };
    let (start, end) = nav_window(window_watermark, today);

    // 首刷优先走单请求全量通道（ADR-0038 决策 6 修订，issue #1062）：一次请求拿
    // 整只基金的历史单位净值，本地裁剪到与分页通道相同的近两年窗口后再用。抓取
    // 失败 / 解析不出序列 / 窗口内无点都 fail-closed 回退分页通道，不静默丢数据；
    // 已有历史序列的增量不触碰本通道（日常增量仍走 lsjz）。
    let full_points = if first_fill {
        match fetch_nav_full_series(&fund.symbol) {
            Ok(points) => {
                let clipped: Vec<NavPoint> = points
                    .into_iter()
                    .filter(|p| {
                        p.date.as_str() >= start.as_str() && p.date.as_str() <= end.as_str()
                    })
                    .collect();
                if clipped.is_empty() {
                    tracing::warn!(
                        code = %fund.symbol,
                        "单请求全量净值通道无窗口内净值，回退分页通道"
                    );
                    None
                } else {
                    Some(clipped)
                }
            }
            Err(error) => {
                tracing::warn!(
                    code = %fund.symbol, %error,
                    "单请求全量净值通道失败，回退分页通道"
                );
                None
            }
        }
    } else {
        None
    };

    // 单请求通道命中即免去分页；否则回退既有分页通道（首刷近两年 / 增量水位次日）。
    let collected = match full_points {
        Some(points) => NavPages {
            points,
            blocked: false,
            truncated: false,
        },
        None => fetch_nav_pages(
            fetch_nav,
            &fund.symbol,
            &start,
            &end,
            MAX_NAV_PAGES,
            on_page,
        )?,
    };

    if collected.points.is_empty() {
        if collected.blocked {
            // 空响应（Data 缺省 / 非对象）= 疑似被拦截或异常：结果不可信，不
            // 得计入成功（issue #1059）。此路记 `inconclusive`——被拦截 ≠
            // 数据源没有可采序列，空态按「待重试」而非「无数据」（issue #1377）。
            tracing::warn!(
                code = %fund.symbol, first_fill, watermark = ?watermark,
                "历史净值返回空响应（疑似被拦截/异常），本轮无法判定是否有新净值"
            );
            return Ok(BackfillOutcome {
                written: false,
                inconclusive: true,
            });
        }
        // 首刷查无净值（查无此码 / 新基金未公布首期）与增量窗口内确实无新净值
        //（T+1 正常空窗）：结局确定——无可采数据 / 已是最新。
        return Ok(BackfillOutcome {
            written: false,
            inconclusive: false,
        });
    }
    if !collected.is_complete() {
        // 本轮窗口不完整（部分页空响应或页数触顶，ADR-0122 决策 8 / issue #1373）：
        // 整只不落库——宁可整只留空重试，不留半根历史。半根历史会让「有历史序列」
        // 冒充「历史完整」，首刷判据（ADR-0038 决策 6）据此失效。结局按待重试。
        tracing::warn!(
            code = %fund.symbol, first_fill,
            blocked = collected.blocked, truncated = collected.truncated,
            "历史净值窗口不完整（部分页空响应或页数触顶），整只不落库待下次重试"
        );
        return Ok(BackfillOutcome {
            written: false,
            inconclusive: true,
        });
    }
    let points = collected.points;

    // 现价 = 窗口内最新公布单位净值；priced_at = nav_date = 净值日期
    // （与 #301 添加基金同形；nav_date 兼任下次同步的水位）。
    // let-else 显式防线（#434，ADR-0060 A 类临时豁免已摘）：points 非空由
    // 前文判空保证，此臂理论不可达；一旦前置防线被移除，此处记警告并跳过
    // 该只、不中断同步。
    let Some(latest) = points.iter().max_by_key(|p| p.date.as_str()) else {
        tracing::warn!(code = %fund.symbol, "净值点意外为空，跳过现价更新");
        return Ok(BackfillOutcome {
            written: false,
            inconclusive: false,
        });
    };

    // 周采样与现价落库同在一只一个事务里（ADR-0122 决策 8 / issue #1373）：单只
    // 标的的历史回填整只一次提交，第 N 个周点写入失败或中途中断整体回滚，磁盘上
    // 不留半根历史——「有历史序列」与「历史完整」由此等价，首刷判据（ADR-0038
    // 决策 6）依赖的正是这个等价。单位净值即价格（ADR-0038 决策 3），与日线共用
    // 降采样与「整周覆盖」幂等（同周重复获取零重复行）。落库经作用域会话短暂取
    // 一次连接（issue #1275）；抓取已在会话之外完成。事务经 [`ensure_transaction`]
    // （ADR-0033 嵌套感知）：连接 autocommit 则自持事务、已在事务中则加入外层。
    // 「一只一事务」的前提是写错误**不被吞**——本函数的写失败一律经 `?` 上抛，
    // 加入外层时由外层持有者回滚，同样整只不留；任一层吞掉写错误才会破坏前提。
    let bars: Vec<KlineBar> = points
        .iter()
        .map(|p| KlineBar {
            date: p.date.clone(),
            close: p.nav,
        })
        .collect();
    session.with_connection(|conn| {
        ensure_transaction(conn, || {
            // 周采样落库（与日 K 回填共用单点），与现价写在同一事务里整只一次提交。
            super::incremental::write_weekly_price_history(
                conn,
                &fund.instrument_id,
                &fund.currency,
                &bars,
            )?;
            upsert_market_price(
                conn,
                &MarketPriceWrite {
                    instrument_id: &fund.instrument_id,
                    price_cents: price_value_to_cents(latest.nav),
                    currency_code: &fund.currency,
                    // 基金现价时点 = 净值日期（现价的行情日期就是净值本身对应的日期）；
                    // nav_date 兼任下次同步的水位。
                    priced_at: &latest.date,
                    nav_date: Some(&latest.date),
                    source: Some(EASTMONEY_PRICE_SOURCE),
                },
            )?;
            Ok(())
        })
    })?;
    Ok(BackfillOutcome {
        written: true,
        inconclusive: false,
    })
}

/// 分页通道的采集结果（ADR-0122 决策 8 / issue #1373）：净值点 + 两个「本轮窗口
/// 不可信」标记——任一为真都让整只不落库、宁可整只留空重试，不留半根历史。
struct NavPages {
    points: Vec<NavPoint>,
    /// 任意一页空响应（报文 `Data` 缺省 / 非对象：疑似被拦截 / 风控）。
    blocked: bool,
    /// 页数触顶（服务端 `TotalCount` 异常，窗口已知未采全，见 [`MAX_NAV_PAGES`]）。
    truncated: bool,
}

impl NavPages {
    /// 本轮窗口是否完整可信：完整才允许整只落库（ADR-0122 决策 8）。
    fn is_complete(&self) -> bool {
        !self.blocked && !self.truncated
    }
}

/// 分页通道取净值点（首刷回退与增量共用）：按服务端总数翻页（页大小为服务端硬
/// 上限），先攒齐全部净值点再一次性降采样——跨页同周的采样必须取最后一个净值日，
/// 逐页落库会用后页的更早日期覆盖前页采样。返回净值点与「本轮窗口不可信」标记
///（空响应 / 页数触顶，见 [`NavPages`]）；页级推进经 `on_page` 透传（issue #1061）：
/// 只在本页抓取返回之后发出（抓取内部的退避/重试等待不产生推进），单页
///（pages ≤ 1）不发。`max_pages` 是页数触顶上限（历史回填取 [`MAX_NAV_PAGES`]，
/// 现价刷新短窗取 [`REFRESH_MAX_NAV_PAGES`]，issue #1377）。
fn fetch_nav_pages<N, P>(
    fetch_nav: &mut N,
    code: &str,
    start: &str,
    end: &str,
    max_pages: u64,
    on_page: &mut P,
) -> Result<NavPages>
where
    N: FnMut(&NavQuery) -> Result<LsjzPage>,
    P: FnMut(u64, u64),
{
    let query = |page: u64| NavQuery {
        code: code.to_string(),
        start_date: start.to_string(),
        end_date: end.to_string(),
        page,
    };
    let first = fetch_nav(&query(1))?;
    let mut blocked = first.blocked;
    let mut points = first.points;
    let raw_pages = first
        .total
        .max(points.len() as u64)
        .div_ceil(LSJZ_PAGE_SIZE);
    let pages = raw_pages.min(max_pages);
    // 触顶即窗口已知未采全（ADR-0122 决策 8）：不在此落已采点，统一由调用方按
    // 「窗口不完整」整只跳过，日志也在那里带出（含触发原因）。
    let truncated = raw_pages > max_pages;
    // 页级推进只在真正翻页时发出；首页在抓取返回、总页数已知后立即报告。
    // 位置固定在 fetch_nav 之后——抓取闭包内部的退避/重试等待不产生推进
    //（issue #1061 的「等待不伪装成推进」由这一先后关系保证）。空响应页
    //（`blocked`，Data 缺省/非对象：疑似被拦截/异常）不算「已回填的一页」，
    // 不推进页码——被风控拦截期间同样不得虚假推进（与 #1059 空响应区分同源）。
    let page_level = pages > 1;
    if page_level && !blocked {
        on_page(1, pages);
    }
    for page in 2..=pages {
        let next = fetch_nav(&query(page))?;
        // 任意一页空响应都让本轮窗口不完整（页 1 空 → 整轮不可信；后续页空 →
        // 已采净值点照常落库、窗口可能缺尾），统一由调用方按形态分流。
        let blocked_page = next.blocked;
        blocked |= blocked_page;
        points.extend(next.points);
        if page_level && !blocked_page {
            on_page(page, pages);
        }
    }
    Ok(NavPages {
        points,
        blocked,
        truncated,
    })
}

/// 取数面命中时逐只净值通道的角色（ADR-0121 取数面 / ADR-0122 决策 2；现价
/// 刷新侧的判据，issue #1377 起首刷不再在现价刷新路径——首刷归后台补全）。
enum BulkDecision<'a> {
    /// 批量面证明没有新净值（最新净值日期不晚于水位）：现价已是最新，整只零请求。
    NothingNew,
    /// 批量面报出比水位更新的当周净值：它的最新单位净值就是当日现价与该周采样点
    /// （周采样要的正是「该周最后一个有报价交易日的价格」），直接落库、整只零请求。
    LandFromBulk(&'a BulkNavPoint),
    /// 逐只通道接管：水位落后批量面最新净值所在自然周超过一周（缺周点补齐——
    /// 批量面的单点落不了中间缺失的周点）或批量面未覆盖（缺口）。
    PerInstrument,
}

/// 取数面命中时的三分类判据（ADR-0122 决策 2「逐只采集只服务首刷与缺周点补齐」
/// 的现价刷新半边，issue #1377）：这是「这次刷新用几次请求」的判定单点——判定为
/// 前两类即零逐只请求，请求量因此不随基金数增长。
fn bulk_decision<'a>(
    latest_hint: Option<&'a BulkNavPoint>,
    watermark: Option<&str>,
) -> BulkDecision<'a> {
    let Some(hint) = latest_hint else {
        return BulkDecision::PerInstrument;
    };
    if hint.is_not_newer_than(watermark) {
        return BulkDecision::NothingNew;
    }
    if week_gap_needs_per_instrument(watermark, &hint.date) {
        return BulkDecision::PerInstrument;
    }
    BulkDecision::LandFromBulk(hint)
}

/// 缺周点补齐判据（ADR-0122 决策 2）：水位落后批量面最新净值所在自然周**超过
/// 一周**（例如应用数周未开）时，中间缺失的周点只有逐只窗口能补——逐只通道接管。
/// 同一周或只差一周（常态的跨周）不触发：批量面的最新净值就是当周采样点，上一周
/// 的采样点已在上一周落库。水位缺失或不可解析时保守接管（宁可多一次请求，
/// 不静默丢周点）。
fn week_gap_needs_per_instrument(watermark: Option<&str>, bulk_date: &str) -> bool {
    let parse = |date: &str| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok();
    let (Some(watermark), Some(bulk_date)) = (watermark.and_then(parse), parse(bulk_date)) else {
        return true;
    };
    super::incremental::week_monday(bulk_date) - super::incremental::week_monday(watermark)
        > chrono::Duration::days(7)
}

/// 把批量取数面的最新单位净值直接落库（ADR-0122 决策 2 / issue #1377）：现价
/// 缓存（现价 = 单位净值、`priced_at` = `nav_date` = 净值日期，与逐只通道同形）
/// 外加当周采样点（每周至多一条、整周覆盖幂等——落库单点与逐只通道共用
/// `upsert_price_history`）。只在批量面报出比水位更新的净值时调用（见
/// [`bulk_decision`]）。
///
/// 当周采样点只落**已有历史序列**的标的：无历史序列者落单点会让「有历史序列」
/// 冒充「历史完整」，永久破坏首刷判据（ADR-0038 决策 6）——首刷的历史由后台
/// 补全整根回填（issue #1377）。
fn land_bulk_point<Q: ScopedSession>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
    hint: &BulkNavPoint,
) -> Result<()> {
    let price_cents = price_value_to_cents(hint.nav);
    session.with_connection(|conn| {
        let has_history = ledger_investment::backfill::has_any_history(conn, &fund.instrument_id)?;
        upsert_market_price(
            conn,
            &MarketPriceWrite {
                instrument_id: &fund.instrument_id,
                price_cents,
                currency_code: &fund.currency,
                priced_at: &hint.date,
                nav_date: Some(&hint.date),
                source: Some(EASTMONEY_PRICE_SOURCE),
            },
        )?;
        if has_history {
            upsert_price_history(
                conn,
                &fund.instrument_id,
                &hint.date,
                price_cents,
                &fund.currency,
                EASTMONEY_PRICE_SOURCE,
            )?;
        }
        Ok(())
    })?;
    Ok(())
}
