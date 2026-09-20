//! 东财历史净值通道（issue #303 / ADR-0038 决策 6）：lsjz 历史净值接口访问、
//! 报文解析、净值同步水位语义与基金分区编排；首刷深回填另走单请求全量通道
//!（基金详情页数据文件，issue #1062）。
//!
//! 货币基金口径（issue #1342 / ADR-0126 决策 3 换源，issue #1563 接线）：货基的
//! 万份收益列不是单位净值（货基单位净值恒 [`MONEY_FUND_UNIT_NAV`]，收益以份额
//! 结转体现）。判定信号已改取证监会基金电子披露的自报形态（确认函数在
//! [`super::csrc`]，接线在逐只刷新与历史首刷两确认点）；本通道的东财自报口径
//!（lsjz `SYType`/`FundType`、详情页数据文件形态）已从同步路径退役——解析层
//! 不再识别与改写，货基行按事实透传（取值位是万份收益），落库由判定门拦截
//! （确认前不落任何取值）。档案与搜索通道的存量口径（建档确认点）未换装，
//! 归 #1568；万份收益序列投影仍服务档案通道的最后一期净值兑底。
//!
//! - 报文解析（[`parse_lsjz`]）与水位窗口（[`nav_window`]）为纯函数，fixture
//!   单测见 `tests/fund_nav.rs`（真实报文形状，不依赖真实网络）；
//! - 页抓取（[`fetch_nav_page`]）复用行情 HTTP 层的主机池 / 重试 / 限流泛型层；
//!   lsjz 为单主机接口且必须携带 Referer 头（缺省被以 ErrCode=-999 拦截）；
//! - 单请求全量通道（[`fetch_nav_full_series`]）一次 GET 基金详情页数据文件、解析
//!   `Data_netWorthTrend` 得整只基金历史单位净值（口径与 lsjz `DWJZ` 逐值一致，
//!   ADR-0038 修订记录有样本验证），仅首刷深回填用——抓取 / 解析失败或窗口内无点
//!   fail-closed 回退 lsjz 分页，不静默丢数据；
//! - 编排按消费职责分两单元（issue #1377 现价与历史解耦收口，issue #1388 拆分
//!   归位）：现价刷新 [`super::fund_price_refresh::refresh_one_fund_price`]（服务
//!   标的信息同步，只刷现价——取数面命中整只零请求，未命中退逐只短窗、页数
//!   封顶）；历史回填 [`super::fund_backfill::backfill_one_fund_history`]（服务
//!   价格历史后台补全）。两单元共用本模块的通道 / 解析 / 水位共享件。

use chrono::NaiveDate;
use rusqlite::params;
use serde::Deserialize;

use ledger_investment::constant_price::{ensure_constant_base_price, mark_constant_unit_price};
use ledger_investment::prices::{EASTMONEY_PRICE_SOURCE, price_value_to_cents};

use ledger_infra::error::{AppError, Result};

use super::channels::FetchFuture;
use super::fund::deserialize_flexible_f64;
use super::http::{Pacer, RetryConfig, request_json_from_hosts, request_text_from_hosts};
use super::session::ScopedSession;

// 历史净值接口：单主机（无公开镜像池），复用行情层的重试与限流泛型层。
const LSJZ_HOSTS: &[&str] = &["https://api.fund.eastmoney.com"];
const LSJZ_PATH: &str = "/f10/lsjz";
/// 每页条数：服务端硬上限（请求更大值实测仍按 20 生效，2026-08），分页循环按此定界。
const LSJZ_PAGE_SIZE: u64 = 20;

// 基金详情页数据文件（pingzhongdata）：单主机（无公开镜像池），一次请求返回整只
// 基金的全部历史净值（issue #1062）。仅服务首刷深回填，增量仍走 lsjz。
pub(super) const PINGZHONG_HOSTS: &[&str] = &["https://fund.eastmoney.com"];
const PINGZHONG_PATH_PREFIX: &str = "/pingzhongdata/";

/// lsjz 整体响应。`TotalCount` 在顶层；`Data` 正常为对象，被拦截形态（缺
/// Referer / 风控）是空字符串，以无标签枚举宽容为 [`NavDataField::Blocked`]；
/// 缺省（Data 字段不存在）为 None。
#[derive(Debug, Deserialize)]
pub(super) struct NavResponse {
    #[serde(rename = "Data", default)]
    pub(super) data: Option<NavDataField>,
    #[serde(rename = "TotalCount", default)]
    pub(super) total_count: u64,
}

/// `Data` 字段的两种 wire 形态（见 [`NavResponse`]）。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum NavDataField {
    Data(NavData),
    /// 被拦截形态兜底（`Data:""` 等）：payload 仅为宽容解析的承接容器，
    /// 解析层不消费其内容（整体按空处理并记 debug 日志）。
    #[allow(dead_code)]
    Blocked(serde_json::Value),
}

#[derive(Debug, Deserialize)]
pub(super) struct NavData {
    #[serde(rename = "LSJZList", default)]
    pub(super) entries: Option<Vec<NavEntry>>,
}

/// 历史净值单行：净值日期 + 单位净值。只解析消费的两列，其余（累计净值、
/// 申购赎回状态等）忽略。
#[derive(Debug, Deserialize)]
pub(super) struct NavEntry {
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

/// 东财基金类型码「货币型」：搜索通道 `FUNDTYPE` 沿用的同一枚代码表
///（issue #1342；建档确认点的存量口径，随 #1568 查询创建接线退役）。
const MONEY_FUND_TYPE_CODE: &str = "005";

/// 货币基金的恒定单位净值（issue #1342）：收益以份额结转体现，单位净值恒为
/// 1.0000——现价与净值序列都按此值收录，万份收益数值不参与。
pub(super) const MONEY_FUND_UNIT_NAV: f64 = 1.0;

/// 按东财基金类型码判定货币基金（issue #1342）：搜索通道（`FUNDTYPE`）的建档
/// 存量口径（随 #1568 查询创建接线退役）。
pub(super) fn is_money_fund_type_code(code: &str) -> bool {
    code.trim() == MONEY_FUND_TYPE_CODE
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

/// 档案通道（基金详情页数据文件）的基金档案：权威名称 + 最后一期单位净值 +
/// 货基形态信号。搜索索引只收在用基金，已终止（清盘）基金被东财摘出该索引
/// （ADR-0039 修订，issue #1212），这两项由档案通道承接；恒定价格信号亦然
/// ——终止货基的类型码已不可达，数据文件形态是它的建档确认面（ADR-0126）。
pub(super) struct FundArchive {
    pub(super) name: String,
    pub(super) last_nav: Option<NavPoint>,
    /// 详情页数据文件缺单位净值序列而有万份收益序列（货基形态，ADR-0126
    /// 决策 3）：建档打标的三处确认源之一。
    pub(super) is_constant_price: bool,
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
    let net_worth = parse_net_worth_trend(js);
    let last_nav = latest_point(net_worth.clone()).or_else(|| {
        // 货基没有单位净值序列：最后一期净值 = 最新收益日 × 恒定单位净值
        // 1.0000（issue #1342）——档案回退报价据此落 1.0000 而非无价。
        latest_point(parse_money_fund_income_series(js))
    });
    // 货基形态信号（ADR-0126 决策 3）：缺单位净值序列（无可用行）而有万份
    // 收益序列——与 last_nav 的回退分支同判，两处消费同一份解析结果。
    let is_constant_price = net_worth.map(|points| points.is_empty()).unwrap_or(true)
        && parse_money_fund_income_series(js).is_some();
    Some(FundArchive {
        name: name.to_string(),
        last_nav,
        is_constant_price,
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
/// [`parse_lsjz`]）。类型名为数据源中立命名（issue #1557）：页形状由通道闭包
/// 签名固定，换源只改通道实现。
#[derive(Debug, Clone, PartialEq)]
pub struct NavPage {
    pub(super) points: Vec<NavPoint>,
    pub(super) total: u64,
    /// 空响应/异常形态（`Data` 缺省或非对象，如缺 Referer 被拦截 / 风控）：
    /// 空结果不可信，不得按「窗口内确实无新净值」计成功（issue #1059）。
    pub(super) blocked: bool,
}

/// 一只基金的单页查询（注入接缝的请求形状）：日期闭区间、页码 1 起。类型名
/// 为数据源中立命名（issue #1557），与页形状 [`NavPage`] 同属分页通道接缝。
#[derive(Debug, Clone, PartialEq)]
pub struct NavQuery {
    pub(super) code: String,
    pub(super) start_date: String,
    pub(super) end_date: String,
    pub(super) page: u64,
}

/// 解析一页 lsjz 报文：挑出有效净值点（日期非空、单位净值 > 0；未公布/异常行
/// 静默过滤，与日线「无效样本不中断」同一姿态）+ 顶层总条数 + 报文形态。被拦截
/// 形态（`Data:""` / [`NavDataField::Blocked`]，含 `Data` 缺省）得空表并标记
/// `blocked`——空表有两种语义（抓取不可信 vs 窗口内确实没有新净值），解析层
/// 负责把它们区分开（issue #1059）。
///
/// 判定不在此层（issue #1563 / ADR-0126 决策 3 换源）：取值列按事实解析，
/// 货基行的 `DWJZ`（万份收益）原样透传——落库由逐只刷新与历史首刷的官方披露
/// 判定门拦截（确认前不落任何取值，万份收益不得冒充单位净值，#1342）。
pub(super) fn parse_lsjz(resp: &NavResponse) -> NavPage {
    let data = match &resp.data {
        Some(NavDataField::Data(data)) => data,
        other => {
            tracing::debug!(payload = ?other, "lsjz Data 缺省或为被拦截形态，按空响应处理");
            return NavPage {
                points: Vec::new(),
                total: resp.total_count,
                blocked: true,
            };
        }
    };
    let points = data
        .entries
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|item| {
            let date = item.fsrq.trim();
            if date.is_empty() {
                return None;
            }
            let nav = item.dwjz.filter(|nav| *nav > 0.0)?;
            Some(NavPoint {
                date: date.to_string(),
                nav,
            })
        })
        .collect();
    NavPage {
        points,
        total: resp.total_count,
        blocked: false,
    }
}

/// 净值同步窗口（ADR-0038 决策 6）：有水位（现价缓存的净值日期）则从水位
/// 次日起（增量）；无水位为首刷，回填近两年。水位非法（理论不可达——写入侧
/// 恒为接口返回的 ISO 日期）按首刷兜底自愈，重复回填由周采样幂等覆盖吸收。
/// 起点只由入参水位决定；「是否首刷」由调用方
///（[`super::fund_backfill::backfill_one_fund_history`]）按有无历史序列判定后传 None
/// 表达（issue #1059），本函数不含那一层判据。
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
pub(super) async fn fetch_nav_page(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    query: &NavQuery,
) -> Result<NavPage> {
    fetch_nav_page_from(client, pacer, query, LSJZ_HOSTS).await
}

/// 同 [`fetch_nav_page`]，主机池可注入（本地 HTTP 服务测试 Referer 传播）。
pub(super) async fn fetch_nav_page_from(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    query: &NavQuery,
    hosts: &[&str],
) -> Result<NavPage> {
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
    let resp: NavResponse = request_json_from_hosts(
        client,
        &params,
        LSJZ_PATH,
        hosts,
        RetryConfig::production(),
        pacer,
        &format!("fetch_nav_page:{}", query.code),
        Some(referer.as_str()),
    )
    .await?;
    Ok(parse_lsjz(&resp))
}

/// 单请求全量净值通道的解析产物：净值点（口径与 lsjz `DWJZ` 一致）。类型名为
/// 数据源中立命名（issue #1557）。货基无单位净值序列，在判定门确认前不会走到
/// 本通道（历史首刷先经官方披露判定，确认即收尾不抓取）。
#[derive(Debug, Clone, PartialEq)]
pub struct FullSeries {
    pub(super) points: Vec<NavPoint>,
}

/// 单请求全量净值通道（issue #1062）：一次 GET 基金详情页数据文件，解析
/// 档案通道取数（issue #1212 / ADR-0039 修订）：一次 GET 基金详情页数据文件，解析
/// 出权威名称与最后一期单位净值；主机池可注入（本地 HTTP 服务测试请求路径与解析）。
/// `Ok(None)` = 这份文件不是本基金的（含无效代码被重定向到错误页的形态），由调用方
/// 按查无此码处置；`Err` 只留给传输类失败（网络 / 限流耗尽重试），与既有「网络失败
/// 上抛」契约一致。
pub(super) async fn fetch_fund_archive_from(
    client: &reqwest::Client,
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
    )
    .await?;
    Ok(parse_fund_archive(&body, code))
}

/// 单请求全量净值通道（issue #1062）：一次 GET 基金详情页数据文件，解析
/// `Data_netWorthTrend` 得整只基金的**全部历史单位净值**——口径与 lsjz 的 `DWJZ`
/// 逐值一致、同落在万分位价格刻度上（spike 四类型样本验证见 ADR-0038 修订记录）。
/// 仅服务首刷深回填，替代约 25 次分页请求；日常增量仍走 lsjz。
///
/// 失败语义是 fail-closed 的前半：网络失败、被拦截（HTML 而非数据文件）或解析不出
/// 单位净值序列都返回 `Err`，调用方据此回退分页通道，不把不可信结果当「无净值」。
/// 货基没有单位净值序列：判定门（官方披露确认）已在抓取前收尾，能走到这里的
/// 都是非货基——万份收益序列的存量投影随东财判定口径退役（issue #1563），
/// 不再为本通道承接货基。
pub(super) async fn fetch_nav_full_series(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    code: &str,
) -> Result<FullSeries> {
    fetch_nav_full_series_from(client, pacer, code, PINGZHONG_HOSTS).await
}

/// 同 [`fetch_nav_full_series`]，主机池可注入（本地 HTTP 服务测试请求路径与解析）。
pub(super) async fn fetch_nav_full_series_from(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    code: &str,
    hosts: &[&str],
) -> Result<FullSeries> {
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
    )
    .await?;
    // 先按单位净值序列解析；解析不出单位净值序列即不可信（货基无该序列，
    // 但判定门已在抓取前收尾，走到这里的非货基缺序列 = 形态漂移或风控页）。
    // 文本通道的解析恒成功，疑似风控页（HTML 而非数据文件）在 HTTP 层看不见
    // ——降速信号由做可信度判定的这一层补上（ADR-0121 决策 5）。
    if let Some(points) = parse_net_worth_trend(&body) {
        return Ok(FullSeries { points });
    }
    pacer.record_throttled();
    Err(AppError::Parse(format!(
        "基金 {code} 详情页数据文件缺少可信的净值序列"
    )))
}

/// 恒定价格标的的打标收尾单点（ADR-0126 决策 3/5；issue #1563 判定门两确认点
/// 与排队竞态窗共用）：回填恒定单位价格标记（单向幂等）并兜底建档常量价
///（1.0000、净值日期空），返回是否实际落价（调用方据此计入价格写入见证）。
/// 取代三处逐字重复的打标块；写入单点 [`ledger_investment::constant_price`]，
/// 本函数不新增第二份落库 SQL。
pub(super) async fn mark_constant_price_on_confirm<Q: ScopedSession>(
    session: &Q,
    instrument_id: &str,
    currency_code: &str,
    priced_at: String,
) -> Result<bool> {
    let cents = price_value_to_cents(MONEY_FUND_UNIT_NAV);
    let instrument_id = instrument_id.to_string();
    let currency_code = currency_code.to_string();
    session
        .with_connection(move |conn| {
            mark_constant_unit_price(conn, &instrument_id, cents)?;
            ensure_constant_base_price(
                conn,
                &instrument_id,
                cents,
                &currency_code,
                &priced_at,
                EASTMONEY_PRICE_SOURCE,
            )
        })
        .await
}

/// 基金分区水位与首刷判据的共享读（issue #1377 自两单元抽出）：水位 = 现价缓存
/// 的净值日期；首刷判据 = 磁盘上没有任何历史序列（issue #1059）。两条读经作用域
/// 会话短暂取一次连接完成（issue #1275）；抓取前不再触碰连接。
pub(super) async fn read_fund_watermark<Q: ScopedSession>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
) -> Result<(Option<String>, bool)> {
    let instrument_id = fund.instrument_id.clone();
    session
        .with_connection(move |conn| {
            let watermark: Option<String> = conn
                .query_row(
                    "SELECT nav_date FROM market_prices WHERE instrument_id=?1",
                    params![instrument_id],
                    |r| r.get(0),
                )
                .ok();
            let has_history = ledger_investment::backfill::has_any_history(conn, &instrument_id)?;
            Ok((watermark, has_history))
        })
        .await
}

/// 分页通道的采集结果（ADR-0122 决策 8 / issue #1373）：净值点 + 两个「本轮窗口
/// 不可信」标记——任一不可信标记为真都让整只不落库、宁可整只留空重试，不留
/// 半根历史。
pub(super) struct NavPages {
    pub(super) points: Vec<NavPoint>,
    /// 任意一页空响应（报文 `Data` 缺省 / 非对象：疑似被拦截 / 风控）。
    pub(super) blocked: bool,
    /// 页数触顶（服务端 `TotalCount` 异常，窗口已知未采全，见 `MAX_NAV_PAGES`）。
    pub(super) truncated: bool,
}

impl NavPages {
    /// 本轮窗口是否完整可信：完整才允许整只落库（ADR-0122 决策 8）。
    pub(super) fn is_complete(&self) -> bool {
        !self.blocked && !self.truncated
    }
}

/// 分页通道取净值点（首刷回退与增量共用）：按服务端总数翻页（页大小为服务端硬
/// 上限），先攒齐全部净值点再一次性降采样——跨页同周的采样必须取最后一个净值日，
/// 逐页落库会用后页的更早日期覆盖前页采样。返回净值点与「本轮窗口不可信」标记
///（空响应 / 页数触顶，见 [`NavPages`]）；页级推进经 `on_page` 透传（issue #1061）：
/// 只在本页抓取返回之后发出（抓取内部的退避/重试等待不产生推进），单页
///（pages ≤ 1）不发。`max_pages` 是页数触顶上限（历史回填取 `MAX_NAV_PAGES`，
/// 现价刷新短窗取 `REFRESH_MAX_NAV_PAGES`，issue #1377）。
pub(super) async fn fetch_nav_pages<N, P>(
    fetch_nav: &mut N,
    code: &str,
    start: &str,
    end: &str,
    max_pages: u64,
    on_page: &mut P,
) -> Result<NavPages>
where
    N: FnMut(&NavQuery) -> FetchFuture<NavPage> + Send,
    P: FnMut(u64, u64) + Send,
{
    let query = |page: u64| NavQuery {
        code: code.to_string(),
        start_date: start.to_string(),
        end_date: end.to_string(),
        page,
    };
    let first = fetch_nav(&query(1)).await?;
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
        let next = fetch_nav(&query(page)).await?;
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
