//! 东财历史净值通道（issue #303 / ADR-0038 决策 6）：lsjz 历史净值接口访问、
//! 报文解析、净值同步水位语义与基金分区编排；首刷深回填另走单请求全量通道
//!（基金详情页数据文件，issue #1062）。
//!
//! - 报文解析（[`parse_lsjz`]）与水位窗口（[`nav_window`]）为纯函数，fixture
//!   单测见 `tests/fund_nav.rs`（真实报文形状，不依赖真实网络）；
//! - 页抓取（[`fetch_nav_page`]）复用行情 HTTP 层的主机池 / 重试 / 限流泛型层；
//!   lsjz 为单主机接口且必须携带 Referer 头（缺省被以 ErrCode=-999 拦截）；
//! - 单请求全量通道（[`fetch_nav_full_series`]）一次 GET 基金详情页数据文件、解析
//!   `Data_netWorthTrend` 得整只基金历史单位净值（口径与 lsjz `DWJZ` 逐值一致，
//!   ADR-0038 修订记录有样本验证），仅首刷深回填用——抓取 / 解析失败或窗口内无点
//!   fail-closed 回退 lsjz 分页，不静默丢数据；
//! - 逐只编排 [`sync_one_fund_nav`] 接受注入的页抓取与单请求抓取两条闭包（生产接
//!   HTTP 层，测试注入 mock）：首刷判据 = **磁盘上没有任何历史序列**（issue #1059
//!   ——添加基金 / AI 导入已把最新净值落现价缓存、水位有值但 PriceHistory 为空，
//!   此时按首刷回填近两年）；已有历史序列的基金以现价缓存的净值日期
//!   （`market_prices.nav_date`，#301 落）为水位，从水位次日起按页增量（常态每只一页，
//!   页大小为服务端硬上限 20）；全部净值点攒齐后一次降采样落周线（跨页同周取最后
//!   一个净值日），现价 = 窗口内最新公布单位净值。基金间的遍历与名称随行刷新、标的级
//!   进度推进归增量同步编排（`incremental`，issue #897 逐只合并推进）；每页抓取返回
//!   后的页级推进经注入回调透传（issue #1061）。

use chrono::NaiveDate;
use rusqlite::{Connection, params};
use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, price_value_to_cents, upsert_market_price,
    upsert_price_history,
};

use super::fund::deserialize_flexible_f64;
use super::http::{KlineBar, Pacer, RetryConfig, request_json_from_hosts, request_text_from_hosts};

// 历史净值接口：单主机（无公开镜像池），复用行情层的重试与限流泛型层。
const LSJZ_HOSTS: &[&str] = &["https://api.fund.eastmoney.com"];
const LSJZ_PATH: &str = "/f10/lsjz";
/// 每页条数：服务端硬上限（请求更大值实测仍按 20 生效，2026-08），分页循环按此定界。
const LSJZ_PAGE_SIZE: u64 = 20;
/// 单只基金单次同步的页数上限：近两年窗口约 25 页（≈500 个净值日 ÷ 20），
/// 上限兜底防异常 TotalCount 导致的失控翻页（触顶记警告日志、保留已采净值点）。
const MAX_NAV_PAGES: u64 = 40;

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
pub(super) struct NavPoint {
    pub(super) date: String,
    pub(super) nav: f64,
}

/// 基金详情页数据文件（pingzhongdata）里单位净值序列的变量名。同文件另有
/// `Data_ACWorthTrend`（累计净值，`[时间戳, 值]` 数组）——本通道刻意只取单位
/// 净值（与历史净值接口 `DWJZ` 同口径，见 [`parse_net_worth_trend`]）。
const NET_WORTH_TREND_VAR: &str = "Data_netWorthTrend";

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
    let array = extract_declared_json_array(js, NET_WORTH_TREND_VAR)?;
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

/// 解析档案通道响应：`fS_code` 与请求代码全等且 `fS_name` 非空才命中（与搜索通道
/// 的 FCODE 全等同一防御纪律）；未声明 / 形态不符 / 代码不符均返回 None，由调用方
/// 按「查无此码」处置。单位净值序列缺失或不可信时按「未取到净值」降级（名称仍可用），
/// 与搜索通道「命中但未公布净值」同形。
pub(super) fn parse_fund_archive(js: &str, code: &str) -> Option<FundArchive> {
    let declared_code = extract_declared_string(js, FUND_CODE_VAR)?;
    if declared_code != code {
        return None;
    }
    let name = extract_declared_string(js, FUND_NAME_VAR)?.trim();
    if name.is_empty() {
        return None;
    }
    let last_nav = parse_net_worth_trend(js)
        .and_then(|points| points.into_iter().max_by(|a, b| a.date.cmp(&b.date)));
    Some(FundArchive {
        name: name.to_string(),
        last_nav,
    })
}

/// 从 JS 文本里取出 `var <name> = "..."` 的字符串字面量（基金名称与代码的实际
/// 形态不含转义，不做转义处理）。未声明 / 缺 `=` / 缺引号均返回 None。
fn extract_declared_string<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let after_name = &text[text.find(name)? + name.len()..];
    let after_eq = &after_name[after_name.find('=')? + 1..];
    let open = after_eq.find('"')?;
    let rest = &after_eq[open + 1..];
    let close = rest.find('"')?;
    Some(&rest[..close])
}

/// 从 JS 文本里取出 `var <name> = [ ... ];` 的数组字面量：先定位变量名后的 `=`，
/// 再从 `[` 做括号配对（跳过 JSON 字符串内的括号与转义）。未声明、缺 `=`、缺 `[`
/// 或括号不闭合均返回 None（上层按不可信数据 fail-closed）。
fn extract_declared_json_array<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let after_name = &text[text.find(name)? + name.len()..];
    let after_eq = &after_name[after_name.find('=')? + 1..];
    let open = after_eq.find('[')?;
    let bytes = after_eq.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, &byte) in bytes.iter().enumerate().skip(open) {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&after_eq[open..=index]);
                }
            }
            _ => {}
        }
    }
    None
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

/// 一页净值的解析结果：有效净值点 + 窗口内总条数（服务端按起止日期过滤后的
/// 总数，供分页循环定界）+ 报文形态（`blocked` = 空响应/被拦截，见
/// [`parse_lsjz`]）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LsjzPage {
    pub(super) points: Vec<NavPoint>,
    pub(super) total: u64,
    /// 空响应/异常形态（`Data` 缺省或非对象，如缺 Referer 被拦截 / 风控）：
    /// 空结果不可信，不得按「窗口内确实无新净值」计成功（issue #1059）。
    pub(super) blocked: bool,
}

/// 一只基金的单页查询（注入接缝的请求形状）：日期闭区间、页码 1 起。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct NavQuery {
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
    let points = data
        .lsjz_list
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|item| {
            let date = item.fsrq.trim();
            let nav = item.dwjz?;
            (nav > 0.0 && !date.is_empty()).then(|| NavPoint {
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
/// 起点只由入参水位决定；「是否首刷」由调用方（[`sync_one_fund_nav`]）按有无
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
    parse_net_worth_trend(&body)
        .ok_or_else(|| AppError::Parse(format!("基金 {code} 详情页数据文件缺少可信的单位净值序列")))
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

/// 单只 fund 标的的净值增量同步（ADR-0038 决策 6；收集面随 issue #827 放开至
/// 库内全部 fund 行）：**首刷判据 = 磁盘上没有任何历史序列**（issue #1059）。
/// 首刷回填近两年——优先走单请求全量通道（详情页数据文件，issue #1062），抓取/
/// 解析失败或窗口内无点则 fail-closed 回退 lsjz 分页通道；已有历史序列的基金走
/// lsjz 分页，以现价缓存的净值日期为水位增量（水位当日不重拉、只取水位次日之后）。
/// 净值点降采样落 PriceHistory（同周整周覆盖幂等），窗口内最新公布净值落现价缓存
/// （现价 = 单位净值、priced_at = nav_date = 净值日期，与 #301 添加基金同形）。
/// 两条抓取闭包由调用方注入（生产接 HTTP 层，测试 mock），本函数不触碰网络；
/// 单只结果累加进调用方的 `stats`；页级推进经注入的 `on_page` 回调透传
///（issue #1061——每页抓取返回后报告「已完成页/总页数」，抓取内部的退避/重试
/// 等待不产生推进）。基金间的遍历、名称随行刷新与标的级进度推进归编排层
///（issue #897）。
///
/// **前置条件**：`fund` 为 6 位真实代码的有通道基金行——名称充代码行（查不到
/// 净值）由调用方计入跳过、零请求（issue #897 起跳过判定与分母口径同收编排层）。
/// 跳过语义：首刷查无净值与**空响应/被拦截**（issue #1059）计入 `skipped`，
/// 不报错不中断；单只网络失败与股票通道一致——上抛中断同步（跳过统计只收
/// 「无法拉取」的行，不含网络失败）。
/// 分页通道取净值点（首刷回退与增量共用）：按服务端总数翻页（页大小为服务端硬
/// 上限），先攒齐全部净值点再一次性降采样——跨页同周的采样必须取最后一个净值日，
/// 逐页落库会用后页的更早日期覆盖前页采样。返回净值点与「任意一页空响应」标记；
/// 页级推进经 `on_page` 透传（issue #1061）：只在本页抓取返回之后发出（抓取内部
/// 的退避/重试等待不产生推进），单页（pages ≤ 1）不发。
fn fetch_nav_pages<N, P>(
    fetch_nav: &mut N,
    code: &str,
    start: &str,
    end: &str,
    on_page: &mut P,
) -> Result<(Vec<NavPoint>, bool)>
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
    let pages = raw_pages.min(MAX_NAV_PAGES);
    if raw_pages > MAX_NAV_PAGES {
        tracing::warn!(code = %code, total = %first.total, "历史净值页数触顶，窗口可能未采全");
    }
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
    Ok((points, blocked))
}

pub(super) fn sync_one_fund_nav<N, S, P>(
    conn: &Connection,
    fund: &super::incremental::SyncInstrument,
    fetch_nav: &mut N,
    fetch_nav_full_series: &mut S,
    stats: &mut FundSyncStats,
    on_page: &mut P,
) -> Result<()>
where
    N: FnMut(&NavQuery) -> Result<LsjzPage>,
    S: FnMut(&str) -> Result<Vec<NavPoint>>,
    // 页级推进回调（issue #1061）：(已完成页, 总页数)。只在本页抓取返回之后发出
    //（抓取内部的退避/重试等待不产生推进）；单页（pages ≤ 1）不发——增量常态
    // 的事件形状与频率不变。
    P: FnMut(u64, u64),
{
    let today = super::incremental::beijing_today();
    // 水位 = 现价缓存的净值日期（股票行恒 NULL，基金行由 #301/本通道写入）。
    let watermark: Option<String> = conn
        .query_row(
            "SELECT nav_date FROM market_prices WHERE instrument_id=?1",
            params![fund.instrument_id],
            |r| r.get(0),
        )
        .ok();
    // 首刷判据 = 磁盘上没有任何历史序列（issue #1059）：添加基金 / AI 导入在
    // 「按代码即拉」时已把最新净值落现价缓存（水位有值）但 PriceHistory 为空，
    // 若以水位作增量起点，增量窗口只剩「水位次日」而近两年回填静默落空
    //（#303 验收在真实账本上未成立的根因）。水位只服务已有历史序列的增量。
    let has_history: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM price_history WHERE instrument_id=?1)",
        params![fund.instrument_id],
        |r| r.get(0),
    )?;
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
    let (points, blocked) = match full_points {
        Some(points) => (points, false),
        None => fetch_nav_pages(fetch_nav, &fund.symbol, &start, &end, on_page)?,
    };

    if points.is_empty() {
        if blocked {
            // 空响应（Data 缺省 / 非对象）= 疑似被拦截或异常：结果不可信，不
            // 得计入成功（issue #1059）。与「窗口内确实无新净值」在统计上分
            // 开——此路计入跳过；日志带出原形态，便于与 T+1 正常空窗对照。
            tracing::warn!(
                code = %fund.symbol, first_fill, watermark = ?watermark,
                "历史净值返回空响应（疑似被拦截/异常），本轮无法判定是否有新净值"
            );
            stats.skipped += 1;
        } else if first_fill {
            // 首刷查无净值（查无此码 / 新基金未公布首期）：无法拉取，计入跳过。
            stats.skipped += 1;
        } else {
            // 增量窗口内确实无新净值（T+1 正常空窗）：现价已是最新，处理成功
            // 但不落库、不计跳过。
            stats.synced += 1;
        }
        return Ok(());
    }
    if blocked {
        // 部分页空响应：已采净值点有效并照常落库，但本轮窗口不完整（先例：
        // 页数触顶同样记警告、保留已采点，见 MAX_NAV_PAGES）。
        tracing::warn!(
            code = %fund.symbol, first_fill,
            "历史净值部分页返回空响应（疑似被拦截/异常），本轮窗口可能未采全"
        );
    }

    // 周采样落库：单位净值即价格（ADR-0038 决策 3），与日线共用降采样与
    // 「整周覆盖」幂等（同周重复获取零重复行）。
    let bars: Vec<KlineBar> = points
        .iter()
        .map(|p| KlineBar {
            date: p.date.clone(),
            close: p.nav,
        })
        .collect();
    for (trade_date, nav) in super::incremental::downsample_weekly(&bars) {
        upsert_price_history(
            conn,
            &fund.instrument_id,
            &trade_date,
            price_value_to_cents(nav),
            &fund.currency,
            EASTMONEY_PRICE_SOURCE,
        )?;
    }
    // 现价 = 窗口内最新公布单位净值；priced_at = nav_date = 净值日期
    // （与 #301 添加基金同形；nav_date 兼任下次同步的水位）。
    // let-else 显式防线（#434，ADR-0060 A 类临时豁免已摘）：points 非空由
    // 前文判空保证，此臂理论不可达；一旦前置防线被移除，此处记警告并跳过
    // 该只、不中断同步。
    let Some(latest) = points.iter().max_by_key(|p| p.date.as_str()) else {
        tracing::warn!(code = %fund.symbol, "净值点意外为空，跳过现价更新");
        return Ok(());
    };
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
    stats.synced += 1;
    stats.written += 1;
    Ok(())
}
