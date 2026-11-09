//! 证监会基金电子披露取数单元（issue #1562 / ADR-0130）：官方场外基金净值披露
//! 的单只基金区间查询与解析——名称、单位净值、累计净值、净值日期，以及货币
//! 基金的自报形态（单位净值为空、万份收益与七日年化有值，ADR-0126 决策 3 换源
//! 后的判定信号）。本单元只产出披露记录，不落库、不接编排（判定接线与查询
//! 创建接线归 #1563 / #1568）。
//!
//! 数据源是法定披露渠道（证监会令第 158 号：开放式基金不晚于每个开放日的次日
//! 披露净值），官方数据但为站点内部接口、无公开契约；已终止（清盘）基金同样
//! 可取（#1212 的 5 只清盘样本与东财档案逐值一致），是已终止基金存在性与最后
//! 一期净值的权威兜底面。仅单只基金区间查询（不支持多代码、日期跨度大于一天
//! 必须携带 6 位代码），逐只礼貌访问，不做全市场扫描。
//!
//! 报文形态（2026-09-20 实测，调研 13.5 节）：
//! - **参数门槛**：DataTables 风格请求必须完整携带 `sEcho` / `iDisplayStart` /
//!   `iDisplayLength` 等参数与业务参数（`fundCode` / `startDate` / `endDate`），
//!   缺少 DataTables 参数时服务端直接返回 500「系统异常」HTML 页、完全不带
//!   `aoData` 时返回 200 空响应——都不是空数据。请求构造收口在 [`ao_data`] 单点，
//!   按站点页面的同款参数全集组装。
//! - **记录形态陷阱**：同一净值日期可能返回两条记录——汇总行（名称不带份额
//!   后缀、取值字段全空）与份额行（名称带 A/C 等后缀、字段有值）。解析必须先
//!   过滤空值行再取份额行，只看第一条会取到空净值。
//! - **货基自报形态**：单位净值为空、`gainPer`（万份收益）与
//!   `yearSevenDayYieldRatePercent`（七日年化，带 % 后缀的字符串）有值。
//!
//! fail-closed 纪律（issue #1562 AC）：响应形状不可信（空响应 / 非 JSON /
//! 500「系统异常」页 / 报文缺 `aaData` / 服务端自报失败 / 声明行数未取全 /
//! 有行但全部未通过解析纪律）一律报 [`malformed_source`] 码化错误，不静默产出空
//! 序列——空序列会让「查无此码」与「数据源坏了」不可分辨，而后者被误判为前者
//! 正是本票明令禁止的。唯一的「无数据」结论来源是结构完好且 `aaData` 为空数组
//! 的可信空报文。
//!
//! 网络请求复用行情 HTTP 层的多主机切换 / 重试 / 限速（文本通道：响应内容是否
//! 可信由解析层判定，文本层不按内容重试，与基金详情页数据文件通道同形）；解析
//! 失败经取数尾部契约（`source_tail`，spec #1675）统一截断 warn、补降速并报
//! 码化错误（ADR-0121 决策 5）。fixture 单测见
//! `tests/csrc.rs`（真实报文形状 + 本地 HTTP 服务钉请求形态）。
//!
//! 判定确认（issue #1563 接线，ADR-0126 决策 3 换源）：[`confirm_money_fund_form_from`]
//! 消费本单元的披露记录回答「这只基金是不是货基」——官方自报形态判定，接线在
//! 逐只刷新、历史首刷（#1563）与基金查询创建（#1568）三处；区间取数的翻页与
//! 完整性核验（[`fetch_fund_nav_series`]）由查询创建接线（#1568）的已终止基金
//! 存在性兜底消费。

use serde::Deserialize;

use ledger_infra::error::{AppError, Result};

use super::fund::deserialize_flexible_string;
use super::http::{Pacer, RetryConfig, request_text_from_hosts};
use super::source_tail;

/// 单元标识（取数尾部契约的 `source` 日志字段）：源畸形 warn 按此分源 grep。
const SOURCE: &str = "csrc-disclosure";

/// 生产主机（证监会基金电子披露网站，官方数据、免注册；站点无 HTTPS 服务，
/// 调研 13.5 节实测）。测试经本地 HTTP 服务注入假响应。
pub(super) const CSRC_HOSTS: &[&str] = &["http://eid.csrc.gov.cn"];

/// 单只基金区间查询接口路径（站点净值信息页的 DataTables 数据源）。
const QUERY_PATH: &str = "/fund/disclose/getPublicFundJZInfoMore.do";

/// 单页行数（`iDisplayLength`）：服务端实测照单全给大分页（近两年约 490 行、
/// 10 年约 2600 行均可单页返回），一页取整段窗口比站点默认的 20 行分页少一个
/// 数量级的请求数——对官方站礼貌访问的形态就是「单只、少请求」。
const PAGE_SIZE: i64 = 500;

/// 页数上限：声明总数超出上限即窗口过深，fail-closed 报错而不是静默截断
///（500 行/页 × 8 页 ≈ 16 年日频净值，覆盖本仓全部净值窗口需求）。
const MAX_PAGES: usize = 8;

/// 非预期响应形状的统一码化错误映射（空响应 / 非 JSON / 500「系统异常」页 / 报文
/// 缺 `aaData` / 服务端自报失败 / 行数取不全 / 有行但全部未通过解析纪律共用
/// 一码；构造时机与形状归取数尾部契约，spec #1675）。具体是哪种形状由契约统一
/// warn 的 error 字段与日志 ctx（`fetch_csrc_nav:{code}`）定位；
/// 不区分错误参数（ADR-0050：params 须 locale 无关）。多数形状的用户补救动作
/// 相同（稍后重试）；窗口过深（声明总数超出页数上限）例外——重试无效、需收窄
/// 查询窗口，窗口语义归调用方，此处不另立用户可见码。
fn malformed_source() -> AppError {
    AppError::coded(
        "sync.disclosure-source-malformed",
        "基金官方披露数据源返回了无法解析的内容，请稍后重试同步",
    )
}

/// 区间查询的一页解析产物：过滤汇总行后的披露记录 + 服务端声明的窗口内总行数
///（`iTotalRecords`，驱动翻页）+ 本页原始行数（翻页完整性核对：声明行数须逐页
/// 取全，取不全即窗口不完整）。记录按服务端原序（净值日期降序，跨页拼接保持
/// 先新后旧）。`total` / `raw` 只被区间取数的翻页面消费（#1568 的兜底臂）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct DisclosurePage {
    pub(super) records: Vec<CsrcNavRecord>,
    pub(super) total: i64,
    pub(super) raw: usize,
}

/// 官方披露的一行有效净值记录（汇总行已过滤、只保留带值份额行）：代码、名称、
/// 净值日期与三类取值字段。取值字段按事实保留（含 0），是否可作价格由消费方
/// 按既有口径裁决；[`CsrcNavRecord::is_money_fund_form`] 是货基自报形态的判定
/// 信号（ADR-0126 决策 3 换源后的确认源）。
#[derive(Debug, Clone, PartialEq)]
pub struct CsrcNavRecord {
    /// 基金代码（6 位）。
    pub code: String,
    /// 披露简称（份额行的名称带份额后缀，如「鹏华安盈宝货币A」）。
    pub name: String,
    /// 净值日期（ISO）。
    pub valuation_date: String,
    /// 单位净值（`shareNetValue`，真实价格值，元）：货基自报形态下为空。
    pub unit_nav: Option<f64>,
    /// 累计净值（`totalNetValue`）：货基自报形态下为空。
    pub accumulated_nav: Option<f64>,
    /// 万份收益（`gainPer`，含 0 与偶发负值）：货基自报形态有值，数值不进价格
    ///（货基单位净值恒定，收益以份额结转体现，ADR-0126）。
    pub gain_per: Option<f64>,
    /// 七日年化收益率（`yearSevenDayYieldRatePercent`，% 后缀剥离为数值）：
    /// 货基自报形态有值。
    pub seven_day_yield_percent: Option<f64>,
}

impl CsrcNavRecord {
    /// 货基自报形态（ADR-0126 决策 3 换源后的判定信号，issue #1563 接线）：
    /// 单位净值为空、万份收益与七日年化有值。
    pub fn is_money_fund_form(&self) -> bool {
        self.unit_nav.is_none() && self.gain_per.is_some() && self.seven_day_yield_percent.is_some()
    }
}

/// 区间查询响应的整体形状：信任锚是 `aaData` 数组在场（空数组 = 可信空）；
/// `success` 为站点自报失败形态（站点 JS 对 `success:false` 弹窗提示）；
/// `iTotalRecords` 驱动翻页。其余字段（`sEcho` 回显、`iTotalDisplayRecords`）
/// 不消费。
#[derive(Debug, Deserialize)]
struct DisclosureResponse {
    #[serde(default)]
    success: Option<bool>,
    #[serde(rename = "iTotalRecords", default)]
    total: i64,
    #[serde(rename = "aaData", default)]
    rows: Option<Vec<DisclosureRow>>,
}

/// 披露记录单行：只解析消费的字段，数值列兼容数字与数字字符串两种 wire 形态、
/// 空串与 null 归「无值」，未知形态按缺省处理（不使整页报文失败）。
#[derive(Debug, Deserialize)]
struct DisclosureRow {
    #[serde(default, deserialize_with = "deserialize_flexible_string")]
    code: Option<String>,
    #[serde(
        rename = "shortName",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    short_name: Option<String>,
    #[serde(
        rename = "valuationDate",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    valuation_date: Option<String>,
    #[serde(
        rename = "shareNetValue",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    share_net_value: Option<String>,
    #[serde(
        rename = "totalNetValue",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    total_net_value: Option<String>,
    #[serde(
        rename = "gainPer",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    gain_per: Option<String>,
    #[serde(
        rename = "yearSevenDayYieldRatePercent",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    seven_day_yield_percent: Option<String>,
}

/// 构造区间查询请求的 `aoData` 参数（DataTables 风格参数全集 + 业务参数，与
/// 站点净值信息页的请求同形）。参数门槛 fail-closed 的前半在这里：请求恒完整，
/// 不存在「缺参数触发 500 系统异常」的形态。`display_start` 为分页起点行数，
/// `echo` 为请求序号（DataTables draw 计数，服务端原样回显）。
fn ao_data(
    code: &str,
    start_date: &str,
    end_date: &str,
    display_start: i64,
    echo: i64,
) -> Result<String> {
    let columns = 5;
    let mut ao = vec![
        serde_json::json!({"name": "sEcho", "value": echo}),
        serde_json::json!({"name": "iColumns", "value": columns}),
        serde_json::json!({"name": "sColumns", "value": ""}),
        serde_json::json!({"name": "iDisplayStart", "value": display_start}),
        serde_json::json!({"name": "iDisplayLength", "value": PAGE_SIZE}),
    ];
    for i in 0..columns {
        ao.push(serde_json::json!({"name": format!("mDataProp_{i}"), "value": ""}));
    }
    ao.push(serde_json::json!({"name": "sSearch", "value": ""}));
    ao.push(serde_json::json!({"name": "bRegex", "value": false}));
    for i in 0..columns {
        ao.push(serde_json::json!({"name": format!("sSearch_{i}"), "value": ""}));
        ao.push(serde_json::json!({"name": format!("bRegex_{i}"), "value": false}));
        ao.push(serde_json::json!({"name": format!("bSortable_{i}"), "value": false}));
    }
    ao.push(serde_json::json!({"name": "fundType", "value": "all"}));
    ao.push(serde_json::json!({"name": "fundCompanyShortName", "value": ""}));
    ao.push(serde_json::json!({"name": "fundCode", "value": code}));
    ao.push(serde_json::json!({"name": "fundName", "value": ""}));
    ao.push(serde_json::json!({"name": "startDate", "value": start_date}));
    ao.push(serde_json::json!({"name": "endDate", "value": end_date}));
    // 纯标量数组的序列化不可达失败；结果不 panic 也不静默产出空参数（空参数
    // 恰会触发官方站的 500 系统异常门槛），按底层错误上抛（ADR-0060）。
    serde_json::to_string(&ao).map_err(|error| AppError::Io(format!("aoData 序列化失败: {error}")))
}

/// 解析一页区间查询报文：过滤汇总行（取值字段全空）与未通过同码/日期纪律的
/// 行，产出带值份额行的披露记录序列。响应不可信（非 JSON / 缺 `aaData` /
/// `success:false`）报 [`malformed_source`]；`aaData` 为空数组是可信空结果；
/// **有行但全部未通过解析纪律**（如 `code` 字段语义漂移导致同码防守整页滤空、
/// 净值日期整体缺失）同样不可信——把它当空窗口会把「数据坏了」误判成
/// 「查无此码」，正是 fail-closed 要防的误判。
///
/// 同码防守（与搜索通道 FCODE 全等、档案通道 `fS_code` 全等同纪律）：区间查询
/// 按单代码圈定，混入的其他代码行不属于本次查询，防御性丢弃。失败回
/// `Err(detail)`（失败原因保留在错误详情，由取数尾部契约统一入日志并补降速）。
pub(super) fn parse_disclosure_page(
    body: &str,
    code: &str,
) -> std::result::Result<DisclosurePage, String> {
    let resp: DisclosureResponse = serde_json::from_str(body)
        .map_err(|error| format!("fundCode={code} 响应不是可信 JSON 报文：{error}"))?;
    if resp.success == Some(false) {
        return Err(format!("fundCode={code} 响应自报失败（success=false）"));
    }
    let Some(rows) = resp.rows else {
        return Err(format!("fundCode={code} 响应缺 aaData 数组（不可信形状）"));
    };
    let records: Vec<_> = rows
        .iter()
        .filter_map(|row| record_from_row(row, code))
        .collect();
    if records.is_empty() && !rows.is_empty() {
        return Err(format!(
            "fundCode={code} 响应有 {} 行但全部未通过解析纪律（不可信形状）",
            rows.len()
        ));
    }
    Ok(DisclosurePage {
        records,
        total: resp.total.max(0),
        raw: rows.len(),
    })
}

/// 单行 → 记录：同码防守、净值日期在场、至少一个取值字段有值（汇总行滤除）
/// 三条纪律全过才产出；取值字段按事实解析（单位/累计净值取正值，万份收益与
/// 七日年化保留任意可解析值——零收益日的 0 是「有值」而非「无值」）。
fn record_from_row(row: &DisclosureRow, code: &str) -> Option<CsrcNavRecord> {
    if row.code.as_deref().map(str::trim) != Some(code) {
        return None;
    }
    let valuation_date = row
        .valuation_date
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())?;
    let unit_nav = positive_value(row.share_net_value.as_deref());
    let accumulated_nav = positive_value(row.total_net_value.as_deref());
    let gain_per = raw_value(row.gain_per.as_deref());
    let seven_day_yield_percent = raw_value(row.seven_day_yield_percent.as_deref());
    if unit_nav.is_none()
        && accumulated_nav.is_none()
        && gain_per.is_none()
        && seven_day_yield_percent.is_none()
    {
        // 汇总行：名称不带份额后缀、取值字段全空——只有带值的份额行参与取值。
        return None;
    }
    Some(CsrcNavRecord {
        code: code.to_string(),
        name: row
            .short_name
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
        valuation_date: valuation_date.to_string(),
        unit_nav,
        accumulated_nav,
        gain_per,
        seven_day_yield_percent,
    })
}

/// 数值列解析（万份收益 / 七日年化口径）：`%` 后缀剥离，任意可解析值（含 0
/// 与负值）都算「有值」；空串 / null / 不可解析归「无值」。
fn raw_value(raw: Option<&str>) -> Option<f64> {
    let raw = raw?.trim();
    let raw = raw.strip_suffix('%').unwrap_or(raw).trim();
    raw.parse::<f64>().ok()
}

/// 净值列解析（单位 / 累计净值口径）：可解析且为正才算「有值」——净值不是
/// 价格意义上的 0 / 负数，非正值按「未披露净值」处理（与既有净值通道的
/// 「无效行过滤」同姿态）。
fn positive_value(raw: Option<&str>) -> Option<f64> {
    raw_value(raw).filter(|value| *value > 0.0)
}

/// 披露查询窗口的回看跨度（判定确认与查询创建的兜底区间共用）：窗口拉宽只为
/// 让**已终止基金**的存量披露落进查询区间（披露止于终止日，终止越深记录越靠
/// 前）；确认与兜底都只消费最新记录，窗口宽度不影响活跃基金的请求数（最新
/// 500 行一页装下约两年日频披露）。
const DISCLOSURE_WINDOW_MONTHS: chrono::Months = chrono::Months::new(120);

/// 披露查询窗口（`YYYY-MM-DD` 闭区间，北京时间今天 − 10 年 → 今天）：判定确认
/// 与查询创建的兜底区间共用同一窗口口径，起点单点。
pub(super) fn disclosure_window_dates() -> (String, String) {
    let today = super::incremental::beijing_today();
    let start = today
        .checked_sub_months(DISCLOSURE_WINDOW_MONTHS)
        .unwrap_or(today);
    (
        start.format("%Y-%m-%d").to_string(),
        today.format("%Y-%m-%d").to_string(),
    )
}

/// 货基判定确认（issue #1563 / ADR-0126 决策 3 换源）：官方披露的自报形态——
/// 窗口内**最新一条**披露记录「单位净值为空、万份收益与七日年化有值」
///（[`CsrcNavRecord::is_money_fund_form`]）即确认。窗口拉宽十年以覆盖已终止
/// 基金的存量披露，但只取最新一页、不翻页：货基自报形态是全序列一致的口径，
/// 最新记录即最近状态；确认不承担取全窗口（那是区间取数的职责），「最新一页」
/// 对任何货基（活跃或已终止）都必然含形态行——货基的每一行披露都带该形态。
///
/// 判定三态由返回值表达：`Ok(true)` = 货基形态确认（调用方据此单向打标）；
/// `Ok(false)` = 缺信号（无披露记录 / 最新记录为普通净值形态）——缺信号不是
/// 「不是恒定标的」的反证，调用方不得据此清空既有标记（ADR-0126 决策 3）；
/// `Err` = 披露源响应不可信（[`malformed_source`]），调用方按本轮不可信处置、
/// 不落任何价格——在信号缺席时落取数面的取值位，正是 #1342 万份收益冒充
/// 单位净值的错法。
///
/// 主机池由调用方传入：生产接通道束注入面的披露面（issue #1674：闭包体不写死
/// 主机，包装常量的第二入口随注入面收口退役），测试注入本地 HTTP 服务测请求
/// 形态与异常响应处置。
pub(super) async fn confirm_money_fund_form_from(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    code: &str,
    hosts: &[&str],
) -> Result<bool> {
    let (start, end) = disclosure_window_dates();
    let page = fetch_disclosure_page(client, pacer, code, &start, &end, hosts, 0).await?;
    Ok(page
        .records
        .iter()
        .max_by(|a, b| a.valuation_date.cmp(&b.valuation_date))
        .is_some_and(|latest| latest.is_money_fund_form()))
}

/// 拉取一只基金的区间披露记录（日期闭区间、按服务端声明总数翻页）：消费面是
/// 基金查询创建的已终止基金存在性兜底（issue #1568，编排单点 [`super::fund`]）。
/// 主机池由调用方传入（生产接 [`CSRC_HOSTS`]，测试注入本地 HTTP 服务钉请求
/// 形态与异常响应处置）。
///
/// 翻页由页 1 声明的 `iTotalRecords` 驱动：声明总数超出页数上限即窗口过深，
/// fail-closed 报错（不静默截断、不发无界请求）；声明行数未逐页取全（翻页被
/// 拦截、报文异常）同样 fail-closed——半截窗口冒充完整数据比慢更糟。
pub(super) async fn fetch_fund_nav_series_from(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    code: &str,
    start_date: &str,
    end_date: &str,
    hosts: &[&str],
) -> Result<Vec<CsrcNavRecord>> {
    tracing::debug!(code, start = %start_date, end = %end_date, "官方披露区间查询");
    let first = fetch_disclosure_page(client, pacer, code, start_date, end_date, hosts, 0).await?;
    let total = first.total;
    // ceil 除法手写（有符号整数的 div_ceil 尚未 stable）；total 已钳非负。
    let page_count = ((total + PAGE_SIZE - 1) / PAGE_SIZE).max(1) as usize;
    if page_count > MAX_PAGES {
        tracing::warn!(
            code,
            total,
            page_size = PAGE_SIZE,
            max = MAX_PAGES,
            "官方披露窗口过深"
        );
        return Err(malformed_source());
    }
    let mut raw = first.raw;
    let mut records = first.records;
    for page in 1..page_count {
        let next = fetch_disclosure_page(
            client,
            pacer,
            code,
            start_date,
            end_date,
            hosts,
            page as i64 * PAGE_SIZE,
        )
        .await?;
        raw += next.raw;
        records.extend(next.records);
    }
    if raw < total as usize {
        tracing::warn!(
            code,
            total,
            fetched = raw,
            "官方披露声明行数未取全，窗口不完整"
        );
        return Err(malformed_source());
    }
    Ok(records)
}

/// 拉取并解析一页（`display_start` 为起点行数）。文本通道的响应内容是否可信
/// 由解析闭包判定；失败经取数尾部契约（[`super::source_tail::finish`]，
/// spec #1675）统一截断 warn、补降速并映射码化错误（ADR-0121 决策 5：
/// 解析失败即数据源异常信号；文本层不按内容重试）。
async fn fetch_disclosure_page(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    code: &str,
    start_date: &str,
    end_date: &str,
    hosts: &[&str],
    display_start: i64,
) -> Result<DisclosurePage> {
    let ao = ao_data(
        code,
        start_date,
        end_date,
        display_start,
        (display_start / PAGE_SIZE) + 1,
    )?;
    let params = [("aoData", ao.as_str())];
    let body = request_text_from_hosts(
        client,
        &params,
        QUERY_PATH,
        hosts,
        RetryConfig::production(),
        pacer,
        &format!("fetch_csrc_nav:{code}"),
        None,
    )
    .await?;
    source_tail::finish(
        SOURCE,
        body.as_bytes(),
        pacer,
        source_tail::utf8,
        |text| parse_disclosure_page(text, code),
        malformed_source,
    )
}
