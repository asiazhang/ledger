//! 新浪场外基金取数单元（ADR-0130 决策 2 / issue #1564）：场外基金净值换源后
//! 的两个取数面——**批量最新净值面**（`hq.sinajs.cn` 的 `f_` 前缀，一次请求携带
//! 多只 6 位基金代码，GBK 编码、必须带 Referer，缺省 403）与**单只全历史面**
//!（`CaihuiFundInfoService.getNav`，一次请求取整只历史单位净值，已终止基金
//! 同样可取）。本单元只取数与解析，不落库、不接 UI。两个面均已接线：**批量面的
//! crate 内消费点**为现价刷新（issue #1565，通道束的批量面闭包）与基金按代码查询
//!（issue #1568，查询创建编排单只 = 一批一条复用本面）；单只全历史面随价格历史
//! 后台补全（issue #1566，通道束的全历史闭包）。
//!
//! **货基行的字段错位（#1342 / ADR-0130 决策 6，本单元的硬约束）**：批量面的
//! 货基行把万份收益放在单位净值位（实测 `f_000198` 单位净值位是 `0.2229`），
//! 七日年化放在累计净值位、前一日单位净值位恒空——不识别即复现「货基市值少
//! 计约七成」。行形态按「前一日单位净值位是否为空」单点判别（2026-09-20 实测
//! 81 只样本：80 只普通行前一日位全部有值，货基行全部为空；已终止货基行同样
//! 为空），错位行显式识别为 [`SinaFundNavForm::MoneyYield`]，经
//! [`SinaFundNavRow::into_nav_point`] 投影时**不产出价格点**——单位净值位的
//! 取值是万份收益，永不被当作单位净值采信。货基的**判定与打标**归证监会披露
//! 自报口径（ADR-0126 决策 3 换源，#1563 接线），本单元只负责不投毒。
//!
//! **全历史面的空序列语义**：`data.data` 空数组是可信空结果（实测：不存在的
//! 代码、参数缺失、货币基金三种形态同形返回空）——它只说明「此面无该基金的
//! 净值序列」，**不**等价「查无此码」：货基退出采集链路（ADR-0126）本就无
//! 序列，存在性与货基判定由官方披露面承接（#1562 / #1568）。已终止基金不受
//! 此限：实测 `002503` 返回终止前全部 1,771 条，末点与官方披露逐值一致。
//!
//! fail-closed：批量面被拦截（缺 Referer 的 403 Forbidden 文本 / 风控 HTML /
//! 空体——报文无任何 `hq_str_f_` 语句）与 GBK 解码出错一律报错，**不**退化为
//! 「零覆盖」——零覆盖会让「今天没有新净值」与「数据源坏了」不可分辨，并把
//! 整场同步静默降级成逐只请求；批量内查无此码的行由数据源以空值语句明示
//!（`var hq_str_f_999999="";`），是可信的逐行缺口。全历史面非 JSON / 缺
//! `result` / 缺 `data` / 缺 `data.data` 数组 / 服务端自报失败（`status.code`
//! 非 0）/ 声明总数未取全 / 有行但全部未通过解析纪律一律报错——半截历史冒充
//! 完整数据比慢更糟（先例：csrc 的窗口完整性纪律）。
//!
//! 报文形态是未公开字段（新浪无公开契约），字段语义表与行形态判别收口于本模
//! 块，漂移时改一处；fixture 单测见 `tests/sina_fund.rs`（2026-09-20 采集的真
//! 实报文 + 本地 HTTP 服务钉请求形态与批量承载量）。

use serde::Deserialize;

use ledger_infra::error::{AppError, Result};

use super::bulk::{BulkNavPoint, FundBatch, FundNameDictionary, FundNavTable};
use super::fund::deserialize_flexible_string;
use super::fund_nav::NavPoint;
use super::http::{
    Pacer, RetryConfig, decode_gbk, request_bytes_from_hosts, request_text_from_hosts,
};

/// 批量最新净值面主机（新浪行情，免注册；必须带 Referer，调研 13.4 节实测）。
/// 测试经本地 HTTP 服务注入假响应。
pub(super) const SINA_FUND_BATCH_HOSTS: &[&str] = &["https://hq.sinajs.cn"];

/// 批量面端点形态是**路径段** `list=f_<代码逗号串>`：
/// `GET https://hq.sinajs.cn/list=f_000001,f_000198`。
pub(super) const SINA_FUND_BATCH_PATH_PREFIX: &str = "/list=";

/// 批量面必须携带的 Referer（模拟新浪财经站跳来源；缺省 HTTP 403，
/// 调研 4.1 / 13.4 节实测）。
pub(super) const SINA_FUND_BATCH_REFERER: &str = "https://finance.sina.com.cn/";

/// 单次请求最多携带的查询键数：批量面上限本质是请求 URI ≈ 8 KB（实测 850 只
/// 7,674 B 成功、900 只 8,124 B → HTTP 431，调研 13.4 节）。`f_` 键每只 10 字节
///（`f_000001,`），750 只最坏请求行 ≈ 7.5 KB，留足余量；超出的代码由
/// [`fetch_sina_fund_nav_rows`] 分批成多次请求。
pub(super) const SINA_FUND_BATCH_SIZE: usize = 750;

/// 单只全历史面主机（新浪基金频道，免注册、无需 Referer，2026-09-20 实测）。
/// 测试经本地 HTTP 服务注入假响应。
pub(super) const SINA_FUND_HISTORY_HOSTS: &[&str] = &["https://stock.finance.sina.com.cn"];

/// 单只全历史面接口路径。
const FUND_NAV_HISTORY_PATH: &str = "/fundInfo/api/openapi.php/CaihuiFundInfoService.getNav";

/// 单只全历史面单页条数：实测照单全给（6,010 条全历史单请求返回，调研 4.1 节）；
/// 上限 10,000 超过国内任一基金的全部历史净值日数（最早 2001 年 → 约 6,010），
/// 单请求即取全。声明总数超出即窗口不完整，fail-closed 报错（不静默截断、
/// 不发无界翻页）。
const FUND_NAV_HISTORY_NUM: &str = "10000";

/// 新浪批量面单行（一只场外基金的最新净值行，字段错位已按形态显式分类）：
/// 代码取自报文键（`hq_str_f_<代码>`），字段语义见 [`SinaFundNavForm`]。
#[derive(Debug, Clone, PartialEq)]
pub struct SinaFundNavRow {
    /// 基金代码（6 位，报文键回显）。
    pub code: String,
    /// 数据源权威名称。
    pub name: String,
    /// 净值日期（ISO，数据源自报）：普通行是该净值日，货基行是收益日。
    pub nav_date: String,
    /// 行形态（普通 / 货基错位）：消费方按形态取值，货基行产不出价格点。
    pub form: SinaFundNavForm,
}

/// 批量面行形态（字段错位的显式判别结果，ADR-0130 决策 6）：
///
/// | 位 | 普通行 | 货基行（错位） |
/// |---|---|---|
/// | 0 | 名称 | 名称 |
/// | 1 | 单位净值 | **万份收益** |
/// | 2 | 累计净值 | 七日年化收益率（%） |
/// | 3 | 前一日单位净值 | 空（判别信号） |
/// | 4 | 净值日期 | 收益日期 |
/// | 5 | 份额规模（语义未确认，不消费） | 同左 |
#[derive(Debug, Clone, PartialEq)]
pub enum SinaFundNavForm {
    /// 普通基金行：单位净值位是真实单位净值（真实价格值，元），累计净值与前
    /// 一日单位净值按事实解出（缺省 None；累计净值不入价，口径同既有通道，
    /// ADR-0038 决策 3 / ADR-0126）。
    UnitNav {
        unit_nav: f64,
        accumulated_nav: Option<f64>,
        prev_unit_nav: Option<f64>,
    },
    /// 货基错位行：单位净值位承载的是万份收益（按事实解出，含 0 与偶发负值；
    /// 含已终止货基的缺收益形态，收益位为空），**不产出价格点**（货基单位净值
    /// 恒 1.0000、退出采集链路，ADR-0126；判定与打标信号归官方披露面，#1563
    /// 接线）。
    MoneyYield {
        gain_per_wan: Option<f64>,
        seven_day_annual_percent: Option<f64>,
    },
}

impl SinaFundNavRow {
    /// 投影为批量取数面的统一载荷 [`BulkNavPoint`]：普通行产出「净值日期 +
    /// 单位净值」；**货基行返回 None**——万份收益不得进入任何价格语义
    ///（ADR-0130 决策 6，类型上封死误用路径）。
    pub fn into_nav_point(self) -> Option<BulkNavPoint> {
        match self.form {
            SinaFundNavForm::UnitNav { unit_nav, .. } => Some(BulkNavPoint {
                date: self.nav_date,
                nav: unit_nav,
            }),
            SinaFundNavForm::MoneyYield { .. } => None,
        }
    }
}

/// 批量面行序列 → 批量取数面载荷（名称 + 最新净值，issue #1565 接线）：普通行
/// 名称与单位净值两处都在场；**货基错位行只有名称**——`into_nav_point` 对它
/// 不产出价格点（ADR-0130 决策 6），于是它只进名称字典、不进净值表。消费方按
/// [`FundBatch::covers`] / [`FundBatch::nav_of`] 区分「面未收录」（缺口，逐条
/// 回退）与「已收录、无价格点」（货基，落逐只臂经官方披露判定门确认收尾）。
///
/// 同一代码出现多条语句时后条覆盖前条（正常报文不会出现；出现即按数据源最后
/// 一条自报取值）。
pub(super) fn fund_batch_from_rows(rows: Vec<SinaFundNavRow>) -> FundBatch {
    let mut names = FundNameDictionary::new();
    let mut nav = FundNavTable::new();
    for row in rows {
        let code = row.code.clone();
        let name = row.name.clone();
        if let Some(point) = row.into_nav_point() {
            nav.insert(code.clone(), point);
        }
        names.insert(code, name);
    }
    FundBatch { names, nav }
}

/// 解析批量最新净值面报文（GBK 已解码的文本）为按响应序的行序列。
///
/// 逐条 `var hq_str_f_<代码>="<字段串>";` 语句解析。查无此码的行由数据源以
/// 空值语句明示，逐行跳过（可信缺口，语义同批一码未被面收录）；名称为空、
/// 字段数不足、净值日期缺失或普通行单位净值非正的行同样逐行跳过——单行异常
/// 是该只的缺口，由调用方按缺口走逐只通道，不是整批失败。整段无任何 `hq_str_f_`
/// 语句（缺 Referer 的 403 Forbidden 文本 / 风控 HTML / 空体）才是不可信形状，
/// 报错 fail-closed。
pub(super) fn parse_sina_fund_nav_rows(body: &str) -> Result<Vec<SinaFundNavRow>> {
    let mut rows = Vec::new();
    let mut saw_statement = false;
    for (code, value) in fund_statements(body) {
        saw_statement = true;
        if let Some(row) = row_from_value(code, value) {
            rows.push(row);
        }
    }
    if !saw_statement {
        return Err(unexpected_batch_response(
            "响应不含任何基金净值语句（疑似被拦截）",
        ));
    }
    Ok(rows)
}

/// 单条语句 → 行：字段语义表与行形态判别的单点（见 [`SinaFundNavForm`]）。
/// 空值语句（查无此码）、名称为空、日期缺失、字段数不足（语义表无法定位）或
/// 普通行单位净值非正/不可解析 → `Ok(None)`（该只按缺口处理）；货基行不要求
/// 收益位有值（已终止货基实测缺收益，形态仍成立）。
fn row_from_value(code: &str, value: &str) -> Option<SinaFundNavRow> {
    let fields: Vec<&str> = value.split(',').collect();
    let name = fields.first()?.trim();
    if value.is_empty() || code.trim().is_empty() || name.is_empty() || fields.len() < 5 {
        return None;
    }
    let nav_date = fields[4].trim();
    if nav_date.is_empty() {
        return None;
    }
    // 行形态判别单点：前一日单位净值位为空 = 货基错位行。普通行的前一日位
    // 实测恒有值（2026-09-20 的 81 只样本），货基行恒空——该位在货基行里没有
    // 「前一日净值」语义可承载。误判的代价不对称：把新发基金首日行（理论可能
    // 前一日位为空）误判成货基只损失一次批量命中（缺口 → 逐只通道兜住），把
    // 货基误判成普通行则把万份收益写进价格（#1342 的七成少计）。
    let form = if fields[3].trim().is_empty() {
        SinaFundNavForm::MoneyYield {
            gain_per_wan: raw_f64(fields[1]),
            seven_day_annual_percent: raw_f64(fields[2]),
        }
    } else {
        SinaFundNavForm::UnitNav {
            unit_nav: positive_f64(fields[1])?,
            accumulated_nav: positive_f64(fields[2]),
            prev_unit_nav: positive_f64(fields[3]),
        }
    };
    Some(SinaFundNavRow {
        code: code.trim().to_string(),
        name: name.to_string(),
        nav_date: nav_date.to_string(),
        form,
    })
}

/// 万份收益解析：任意可解析值（含 0 与偶发负值）都算「有值」，空串 / 不可解析
/// 归「无值」（已终止货基实测缺收益，形态仍成立）。数值不进价格。
fn raw_f64(field: &str) -> Option<f64> {
    field.trim().parse::<f64>().ok()
}

/// 净值位解析（单位 / 累计 / 前一日口径）：可解析且为正才算「有值」——净值不是
/// 价格意义上的 0 / 负数（与 csrc 的净值列同姿态）。
fn positive_f64(field: &str) -> Option<f64> {
    raw_f64(field).filter(|value| *value > 0.0)
}

/// 从报文体里取出全部 `var hq_str_f_<代码>="<字段串>";` 语句。报文一行一条但
/// 按标记扫描而非按行切分（对换行 / 前导 `var ` 变体宽容）；畸形片段跳过，
/// 不中断后续语句。
fn fund_statements(body: &str) -> Vec<(&str, &str)> {
    const MARKER: &str = "hq_str_f_";
    let mut statements = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = body[cursor..].find(MARKER) {
        let rest = &body[cursor + offset + MARKER.len()..];
        let Some(eq) = rest.find('=') else { break };
        let code = rest[..eq].trim();
        let Some(value) = rest[eq + 1..].strip_prefix('"') else {
            cursor += offset + MARKER.len();
            continue;
        };
        let Some(end) = value.find('"') else { break };
        statements.push((code, &value[..end]));
        cursor += offset + MARKER.len() + eq + 2 + end + 1;
    }
    statements
}

/// 拉取一批场外基金的最新净值行（分批，单请求最多 [`SINA_FUND_BATCH_SIZE`]
/// 只），合并各批结果为按请求序的行序列。主机池由调用方传入（生产接
/// [`SINA_FUND_BATCH_HOSTS`]，测试经本地 HTTP 服务注入假响应，先例：tencent）。
pub(super) async fn fetch_sina_fund_nav_rows(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    codes: &[String],
) -> Result<Vec<SinaFundNavRow>> {
    let mut rows = Vec::new();
    for chunk in codes.chunks(SINA_FUND_BATCH_SIZE) {
        // 空白代码不构造查询键（请求侧跳过该查询单元，与 tencent 的市场未知
        // 键同姿态）。
        let keys: Vec<String> = chunk
            .iter()
            .filter_map(|code| {
                let code = code.trim();
                (!code.is_empty()).then(|| format!("f_{code}"))
            })
            .collect();
        if keys.is_empty() {
            continue;
        }
        rows.extend(fetch_sina_fund_batch(client, pacer, hosts, &keys.join(",")).await?);
    }
    Ok(rows)
}

/// 单批请求：请求行 `GET /list=f_<逗号串>` + Referer，GBK 解码后按
/// [`parse_sina_fund_nav_rows`] 解析。解析失败按疑似拦截补降速信号（文本形状
/// 判定在 HTTP 层看不见，先例：tencent 的批量面）。
async fn fetch_sina_fund_batch(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    keys: &str,
) -> Result<Vec<SinaFundNavRow>> {
    tracing::debug!(
        keys_count = keys.split(',').count(),
        "新浪场外基金批量净值查询"
    );
    let path = format!("{SINA_FUND_BATCH_PATH_PREFIX}{keys}");
    let bytes = request_bytes_from_hosts(
        client,
        &[],
        &path,
        hosts,
        RetryConfig::production(),
        pacer,
        "fetch_sina_fund_nav_rows",
        Some(SINA_FUND_BATCH_REFERER),
    )
    .await?;
    // GBK 解码与报文形状两道判据都归本层：任一失败都补降速信号
    //（ADR-0121 决策 5，先例：tencent 的批量面）。解码错误统一包装为本单元
    // 的非预期响应错误（解码原语归 HTTP 层单点，单元上下文在此补齐）。
    decode_gbk(&bytes)
        .map_err(unexpected_batch_response)
        .and_then(|body| parse_sina_fund_nav_rows(&body))
        .map_err(|error| {
            tracing::warn!(%error, "新浪场外基金批量净值响应不可信");
            pacer.record_throttled();
            error
        })
}

/// 非预期批量面形状的统一错误（被拦截 / 截断 / 无语句）：退出取数与解析，不
/// 回退为空序列。文案内部化（用户可见面是编排的「已降级、本次较慢」）。
fn unexpected_batch_response(detail: impl std::fmt::Display) -> AppError {
    AppError::Parse(format!("新浪场外基金批量净值响应不可解析：{detail}"))
}

/// 单只全历史面的单行：只解析消费的两列——净值日期（`fbrq`，wire 形态
/// `2026-09-18 00:00:00`）与单位净值（`jjjz`，数字字符串）；累计净值（`ljjz`）
/// 在报文但在场不消费（口径同既有通道：单位净值即价格，累计净值不入价，
/// ADR-0038 决策 3 / ADR-0126）。
#[derive(Debug, Deserialize)]
struct NavHistoryRow {
    #[serde(default, deserialize_with = "deserialize_flexible_string")]
    fbrq: Option<String>,
    #[serde(default, deserialize_with = "deserialize_flexible_string")]
    jjjz: Option<String>,
}

/// 全历史面 `data` 对象：`data.data` 数组是信任锚（在场即结构可信，空数组 =
/// 可信空），`total_num` 驱动完整性核对（字符串形态，实测 `"6010"`）。
#[derive(Debug, Deserialize)]
struct NavHistoryData {
    #[serde(rename = "data", default)]
    rows: Option<Vec<NavHistoryRow>>,
    #[serde(
        rename = "total_num",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    total_num: Option<String>,
}

/// 全历史面响应整体形状：`result` 缺失即不可信（非 JSON / 被拦截）；
/// `status.code` 服务端自报失败形态（实测恒 0，非 0 视为源自报失败）。
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // #1566 单只全历史面接线前豁免（接线时连同本注记撤去）
struct NavHistoryResponse {
    #[serde(default)]
    result: Option<NavHistoryResult>,
}

#[derive(Debug, Deserialize)]
struct NavHistoryResult {
    #[serde(default)]
    status: Option<NavHistoryStatus>,
    #[serde(default)]
    data: Option<NavHistoryData>,
}

#[derive(Debug, Deserialize)]
struct NavHistoryStatus {
    #[serde(default, deserialize_with = "deserialize_flexible_string")]
    code: Option<String>,
}

/// 解析单只全历史面报文为净值序列（wire 序：净值日期降序，先新后旧；消费端
/// 采样自带按日排序，序无关）。响应不可信（缺 `result` / 缺 `data` /
/// 缺 `data.data` 数组 / `status.code` 非 0 / 声明总数未取全 / 有行但全部
/// 未通过解析纪律）报错 fail-closed；`data.data` 空数组是可信空结果（查无此码 /
/// 货基 / 窗口外，见模块文档的空序列语义）。
///
/// 行纪律：日期取 `fbrq` 的日期部分并按 ISO 校验、单位净值（`jjjz`）为正才
/// 产出（无效行静默过滤，与日线「无效样本不中断」同姿态）。
pub(super) fn parse_fund_nav_history(body: &str) -> Result<Vec<NavPoint>> {
    let resp: NavHistoryResponse = serde_json::from_str(body).map_err(|error| {
        tracing::warn!(error = %error, head = %body_head(body), "新浪基金历史净值响应不是可信 JSON 报文");
        unexpected_history_response()
    })?;
    parse_fund_nav_history_response(resp)
}

/// 已反序列化响应的结构纪律（与 [`parse_fund_nav_history`] 同判，由它取出
/// 报文里的响应对象后委托本函数收口）。
fn parse_fund_nav_history_response(resp: NavHistoryResponse) -> Result<Vec<NavPoint>> {
    let result = resp.result.ok_or_else(unexpected_history_response)?;
    if result
        .status
        .and_then(|status| status.code)
        .is_some_and(|code| code != "0")
    {
        tracing::warn!("新浪基金历史净值响应自报失败（status.code 非 0）");
        return Err(unexpected_history_response());
    }
    let data = result.data.ok_or_else(unexpected_history_response)?;
    let rows = data.rows.ok_or_else(unexpected_history_response)?;
    let points: Vec<NavPoint> = rows
        .iter()
        .filter_map(|row| {
            // 日期取 `fbrq` 的日期部分（`2026-09-18 00:00:00` → `2026-09-18`）。
            let date = row.fbrq.as_deref()?.split(' ').next()?;
            if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
                return None;
            }
            let nav = positive_f64(row.jjjz.as_deref().unwrap_or_default())?;
            Some(NavPoint {
                date: date.to_string(),
                nav,
            })
        })
        .collect();
    if points.is_empty() && !rows.is_empty() {
        tracing::warn!(
            raw = rows.len(),
            "新浪基金历史净值响应有行但全部未通过解析纪律（不可信形状）"
        );
        return Err(unexpected_history_response());
    }
    // 完整性核对：单请求按 FUND_NAV_HISTORY_NUM 取全，服务端声明的原始行数
    // 超出实际收到的行数即窗口被截断（源侧上限漂移 / 翻页形态变化），fail-
    // closed——半截历史冒充完整数据比慢更糟。核对锚在**原始行数**而非纪律
    // 过滤后的有效行：行纪律（坏行过滤）与窗口完整性（声明行数取全）是两件
    // 事，一条坏行不该让整只历史取不回。
    if let Some(total) = data
        .total_num
        .as_deref()
        .and_then(|t| t.trim().parse::<usize>().ok())
        && rows.len() < total
    {
        tracing::warn!(
            total,
            fetched = rows.len(),
            "新浪基金历史净值声明总数未取全，窗口不完整"
        );
        return Err(unexpected_history_response());
    }
    Ok(points)
}

/// 拉取一只基金的全历史单位净值。主机池由调用方传入（生产接
/// [`SINA_FUND_HISTORY_HOSTS`]，测试注入假响应）。`datefrom` / `dateto` 为 ISO
/// 日期窗口（None = 不限，一次请求取整只历史，含已终止基金的末点）。两参是
/// 端点 wire 契约的组成部分（实测请求形态携空值，fixture 钉住），当前唯一
/// 调用方（通道束全历史闭包）恒传 None：窗口语义（首刷近两年 / 水位次日
/// 增量）由消费方本地裁剪，不依赖服务端窗口过滤行为（issue #1566）。报文体按
/// 原始文本取回（UTF-8 解码归重试层），解析与可信度判定收口在
/// [`parse_fund_nav_history`]。
pub(super) async fn fetch_fund_nav_history(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    code: &str,
    datefrom: Option<&str>,
    dateto: Option<&str>,
) -> Result<Vec<NavPoint>> {
    tracing::debug!(code, "新浪基金全历史净值查询");
    let params = [
        ("symbol", code.trim()),
        ("datefrom", datefrom.unwrap_or("")),
        ("dateto", dateto.unwrap_or("")),
        ("page", "1"),
        ("num", FUND_NAV_HISTORY_NUM),
    ];
    let body = request_text_from_hosts(
        client,
        &params,
        FUND_NAV_HISTORY_PATH,
        hosts,
        RetryConfig::production(),
        pacer,
        &format!("fetch_sina_fund_history:{code}"),
        None,
    )
    .await?;
    parse_fund_nav_history(&body).inspect_err(|_| {
        // 文本通道的网络/解码失败已在重试层补降速信号；报文形状判据在本层，
        // 失败同样补记（文本可信度由解析层判定，先例：csrc 的区间查询）。
        pacer.record_throttled()
    })
}

/// 非预期全历史形状的统一错误（非 JSON / 缺结构 / 自报失败 / 截断）：退出解析
/// 不产出半截序列。文案内部化（消费面是历史补全的回退与查询创建的失败分流）。
fn unexpected_history_response() -> AppError {
    AppError::Parse("新浪基金历史净值响应不可解析".into())
}

/// 日志用的响应头片段（截断，避免日志吞下整页 HTML）。
fn body_head(body: &str) -> String {
    body.chars().take(120).collect()
}
