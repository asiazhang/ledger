//! 东财历史净值通道（lsjz 分页，issue #303 / ADR-0038 决策 6）：报文访问、
//! 解析、净值同步水位语义与基金分区共享件。
//!
//! **历史回填已换源**（issue #1566 / ADR-0130 决策 2）：首刷深回填与缺周点补齐
//! 改走新浪单只全历史面（[`super::sina_fund`]），本模块的 lsjz 分页通道只剩
//! 现价刷新的逐只短窗一个消费者（issue #1377）；原首刷专用的单请求全量通道
//!（基金详情页数据文件，issue #1062）随换源退役删除。建档确认点的档案通道
//!（同一份数据文件，issue #1212 / ADR-0039 修订）与东财搜索建议面随基金查询
//! 创建换源退役（issue #1568）——基金详情页数据文件的解析共享件（单位净值序列
//! 解析与 JS 字面量提取）随之整体删除，ADR-0130 决策 1：不留「停用但可启用」
//! 的死代码。本模块只剩 lsjz 报文解析、水位窗口与共享写路径件。
//!
//! 货币基金口径（issue #1342 / ADR-0126 决策 3 换源，issue #1563 接线）：货基的
//! 万份收益列不是单位净值（货基单位净值恒 [`MONEY_FUND_UNIT_NAV`]，收益以份额
//! 结转体现）。判定信号已改取证监会基金电子披露的自报形态（确认函数在
//! [`super::csrc`]，接线在逐只刷新与历史首刷两确认点）；本通道的东财自报口径
//!（lsjz `SYType`/`FundType`）已从同步路径退役——解析层不再识别与改写，货基行
//! 按事实透传（取值位是万份收益），落库由判定门拦截（确认前不落任何取值）。
//! 建档路径的档案/搜索存量口径已随基金查询创建换源退役（issue #1568），万份
//! 收益序列投影随之删除。
//!
//! - 报文解析（[`parse_lsjz`]）与水位窗口（[`nav_window`]）为纯函数，fixture
//!   单测见 `tests/fund_nav.rs`（真实报文形状，不依赖真实网络）；
//! - 页抓取（[`fetch_nav_page`]）复用行情 HTTP 层的主机池 / 重试 / 限流泛型层；
//!   lsjz 为单主机接口且必须携带 Referer 头（缺省被以 ErrCode=-999 拦截）；
//! - 编排按消费职责分两单元（issue #1377 现价与历史解耦收口，issue #1388 拆分
//!   归位）：现价刷新 [`super::fund_price_refresh::refresh_one_fund_price`]（服务
//!   标的信息同步，只刷现价——取数面命中整只零请求，未命中退逐只短窗、页数
//!   封顶）；历史回填 [`super::fund_backfill::backfill_one_fund_history`]（服务
//!   价格历史后台补全，取数走新浪全历史面）。现价刷新共用本模块的通道 / 解析 /
//!   水位共享件。

use chrono::NaiveDate;
use rusqlite::params;
use serde::Deserialize;

use ledger_investment::constant_price::{ensure_constant_base_price, mark_constant_unit_price};
use ledger_investment::prices::{CSRC_PRICE_SOURCE, price_value_to_cents};

use ledger_infra::error::Result;

use super::channels::FetchFuture;
use super::fund::deserialize_flexible_f64;
use super::http::{Pacer, RetryConfig, request_json_from_hosts};
use super::session::ScopedSession;

// 历史净值接口：单主机（无公开镜像池），复用行情层的重试与限流泛型层。
const LSJZ_HOSTS: &[&str] = &["https://api.fund.eastmoney.com"];
const LSJZ_PATH: &str = "/f10/lsjz";
/// 每页条数：服务端硬上限（请求更大值实测仍按 20 生效，2026-08），分页循环按此定界。
const LSJZ_PAGE_SIZE: u64 = 20;

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

/// 货币基金的恒定单位净值（issue #1342）：收益以份额结转体现，单位净值恒为
/// 1.0000——现价与恒定价兜底行都按此值收录，万份收益数值不参与。
pub(super) const MONEY_FUND_UNIT_NAV: f64 = 1.0;

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

impl NavPage {
    /// 空页（窗口内确实无新净值；`total=0`、非 `blocked`）：通道束为壳层注入接缝
    /// 的公开面，桩实现方需要能命名与构造应答形状（与 [`super::channels::QuoteItem`]
    /// 同款，issue #1276）；生产侧页不由此构造。
    pub fn empty() -> Self {
        Self {
            points: vec![],
            total: 0,
            blocked: false,
        }
    }
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
                // 恒定价格的确认源是官方披露自报形态（issue #1563 / #1568，
                // ADR-0130 决策 7：来源键随换源如实记 csrc）。
                CSRC_PRICE_SOURCE,
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
    /// 页数触顶（服务端 `TotalCount` 异常，窗口已知未采全，见消费方各自的
    /// `max_pages` 实参）。
    pub(super) truncated: bool,
}

impl NavPages {
    /// 本轮窗口是否完整可信：完整才允许整只落库（ADR-0122 决策 8）。
    pub(super) fn is_complete(&self) -> bool {
        !self.blocked && !self.truncated
    }
}

/// 分页通道取净值点（现价刷新逐只短窗专用，issue #1377；历史回填已换源新浪
/// 全历史面、不再消费本通道，issue #1566）：按服务端总数翻页（页大小为服务端硬
/// 上限），先攒齐全部净值点再一次性降采样——跨页同周的采样必须取最后一个净值日，
/// 逐页落库会用后页的更早日期覆盖前页采样。返回净值点与「本轮窗口不可信」标记
///（空响应 / 页数触顶，见 [`NavPages`]）；页级推进经 `on_page` 透传（issue #1061）：
/// 只在本页抓取返回之后发出（抓取内部的退避/重试等待不产生推进），单页
///（pages ≤ 1）不发。`max_pages` 是页数触顶上限（现价刷新短窗取
/// `REFRESH_MAX_NAV_PAGES`，issue #1377）。
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
