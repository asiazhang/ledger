//! 东财历史净值通道（issue #303 / ADR-0038 决策 6）：lsjz 历史净值接口访问、
//! 报文解析、净值同步水位语义与基金分区编排。
//!
//! - 报文解析（[`parse_lsjz`]）与水位窗口（[`nav_window`]）为纯函数，fixture
//!   单测见 `tests/fund_nav.rs`（真实报文形状，不依赖真实网络）；
//! - 页抓取（[`fetch_nav_page`]）复用行情 HTTP 层的主机池 / 重试 / 限流泛型层；
//!   lsjz 为单主机接口且必须携带 Referer 头（缺省被以 ErrCode=-999 拦截）；
//! - 分区编排 [`sync_fund_navs`] 接受注入的页抓取闭包（生产接 HTTP 层，测试
//!   注入 mock），以现价缓存的净值日期（`market_prices.nav_date`，#301 落）为
//!   水位：首刷（无水位）回填近两年、此后从水位次日起按页增量（常态每只一页，
//!   页大小为服务端硬上限 20）；全部净值点攒齐后一次降采样落周线（跨页同周取
//!   最后一个净值日），现价 = 窗口内最新公布单位净值。

use chrono::NaiveDate;
use rusqlite::{Connection, params};
use serde::Deserialize;

use crate::commands::investment::is_six_digit_code;
use crate::error::Result;

use super::fund::deserialize_flexible_f64;
use super::http::{KlineBar, Pacer, RetryConfig, request_json_from_hosts};
use super::persist::{price_value_to_cents, upsert_market_price, upsert_price_history};

// 历史净值接口：单主机（无公开镜像池），复用行情层的重试与限流泛型层。
const LSJZ_HOSTS: &[&str] = &["https://api.fund.eastmoney.com"];
const LSJZ_PATH: &str = "/f10/lsjz";
/// 每页条数：服务端硬上限（请求更大值实测仍按 20 生效，2026-08），分页循环按此定界。
const LSJZ_PAGE_SIZE: u64 = 20;
/// 单只基金单次同步的页数上限：近两年窗口约 25 页（≈500 个净值日 ÷ 20），
/// 上限兜底防异常 TotalCount 导致的失控翻页（触顶记警告日志、保留已采净值点）。
const MAX_NAV_PAGES: u64 = 40;

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

/// 单页抓取结果：解析后的净值点 + 窗口内总条数（服务端按起止日期过滤后的
/// 总数，供分页循环定界）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LsjzPage {
    pub(super) points: Vec<NavPoint>,
    pub(super) total: u64,
}

/// 一只基金的单页查询（注入接缝的请求形状）：日期闭区间、页码 1 起。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct NavQuery {
    pub(super) code: String,
    pub(super) start_date: String,
    pub(super) end_date: String,
    pub(super) page: u64,
}

/// 从 lsjz 报文挑出有效净值点：日期非空、单位净值 > 0（未公布/异常行静默
/// 过滤，与日线「无效样本不中断」同一姿态）。被拦截形态（`Data:""`)得空表。
pub(super) fn parse_lsjz(resp: &LsjzResponse) -> Vec<NavPoint> {
    let data = match &resp.data {
        Some(LsjzDataField::Data(data)) => data,
        other => {
            tracing::debug!(payload = ?other, "lsjz Data 缺省或为被拦截形态，按空处理");
            return Vec::new();
        }
    };
    data.lsjz_list
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
        .collect()
}

/// 净值同步窗口（ADR-0038 决策 6）：有水位（现价缓存的净值日期）则从水位
/// 次日起（增量）；无水位为首刷，回填近两年。水位非法（理论不可达——写入侧
/// 恒为接口返回的 ISO 日期）按首刷兜底自愈，重复回填由周采样幂等覆盖吸收。
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
    Ok(LsjzPage {
        total: resp.total_count,
        points: parse_lsjz(&resp),
    })
}

/// 基金分区的同步统计（与 [`super::incremental`] 的股票统计同源汇总）：
/// `synced` = 处理成功（含「已是最新、无新净值」）；`skipped` = 无法拉取
/// （非 6 位代码 / 首刷查无净值）；`written` = 实际落库净值的只数（价格失效
/// 信号判定依据，零变化不广播）。
pub(super) struct FundSyncStats {
    pub(super) synced: usize,
    pub(super) skipped: usize,
    pub(super) written: usize,
}

/// 全部 fund 持仓标的的净值增量同步（ADR-0038 决策 6）：逐只请求 lsjz，以
/// 现价缓存的净值日期为水位增量回填——净值点降采样落 PriceHistory（同周
/// 整周覆盖幂等），窗口内最新公布净值落现价缓存（现价 = 单位净值、
/// priced_at = nav_date = 净值日期，与 #301 添加基金同形）。页抓取闭包由
/// 调用方注入（生产接 HTTP 层，测试 mock），本函数不触碰网络。
///
/// 跳过语义：非 6 位代码（名称充代码等无真实代码的行，查不到净值）与首刷
/// 查无净值计入 `skipped`，不报错不中断；单只网络失败与股票通道一致——上抛
/// 中断同步（跳过统计只收「无法拉取」的行，不含网络失败）。
pub(super) fn sync_fund_navs<N>(
    conn: &Connection,
    funds: &[&super::incremental::HeldInstrument],
    fetch_nav: &mut N,
) -> Result<FundSyncStats>
where
    N: FnMut(&NavQuery) -> Result<LsjzPage>,
{
    let today = super::incremental::beijing_today();
    let mut stats = FundSyncStats {
        synced: 0,
        skipped: 0,
        written: 0,
    };
    for fund in funds {
        if !is_six_digit_code(&fund.symbol) {
            stats.skipped += 1;
            continue;
        }
        // 水位 = 现价缓存的净值日期（股票行恒 NULL，基金行由 #301/本通道写入）。
        let watermark: Option<String> = conn
            .query_row(
                "SELECT nav_date FROM market_prices WHERE instrument_id=?1",
                params![fund.instrument_id],
                |r| r.get(0),
            )
            .ok();
        let (start, end) = nav_window(watermark.as_deref(), today);
        let query = |page: u64| NavQuery {
            code: fund.symbol.clone(),
            start_date: start.clone(),
            end_date: end.clone(),
            page,
        };
        // 按服务端总数翻页（页大小为服务端硬上限）；先攒齐全部净值点再一次性
        // 降采样——跨页同周的采样必须取最后一个净值日，逐页落库会用后页的
        // 更早日期覆盖前页采样。
        let first = fetch_nav(&query(1))?;
        let mut points = first.points;
        let raw_pages = first
            .total
            .max(points.len() as u64)
            .div_ceil(LSJZ_PAGE_SIZE);
        let pages = raw_pages.min(MAX_NAV_PAGES);
        if raw_pages > MAX_NAV_PAGES {
            tracing::warn!(code = %fund.symbol, total = %first.total, "历史净值页数触顶，窗口可能未采全");
        }
        for page in 2..=pages {
            points.extend(fetch_nav(&query(page))?.points);
        }

        if points.is_empty() {
            if watermark.is_some() {
                // 增量窗口内无新净值：现价已是最新，处理成功但不落库、不计跳过。
                stats.synced += 1;
            } else {
                // 首刷查无净值（查无此码 / 新基金未公布首期）：无法拉取，计入跳过。
                stats.skipped += 1;
            }
            continue;
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
            )?;
        }
        // 现价 = 窗口内最新公布单位净值；priced_at = nav_date = 净值日期
        // （与 #301 添加基金同形；nav_date 兼任下次同步的水位）。
        let latest = points
            .iter()
            .max_by_key(|p| p.date.as_str())
            .expect("points 非空，前文已判空");
        upsert_market_price(
            conn,
            &fund.instrument_id,
            price_value_to_cents(latest.nav),
            &fund.currency,
            &latest.date,
            Some(&latest.date),
        )?;
        stats.synced += 1;
        stats.written += 1;
    }
    Ok(stats)
}
