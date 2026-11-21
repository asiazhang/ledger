use serde::Deserialize;

use ledger_infra::error::{AppError, Result};

use super::bulk::{BulkNavPoint, FundBatch, FundNameDictionary, FundNavTable};
use super::fund::deserialize_flexible_string;
use super::fund_nav::NavPoint;
use super::http::{
    Pacer, RetryConfig, decode_gbk, request_bytes_from_hosts, request_text_from_hosts,
};
use super::source_tail;

const SOURCE_BATCH: &str = "sina-batch-nav";
const SOURCE_HISTORY: &str = "sina-nav-history";

pub(super) const SINA_FUND_BATCH_HOSTS: &[&str] = &["https://hq.sinajs.cn"];

pub(super) const SINA_FUND_BATCH_PATH_PREFIX: &str = "/list=";

pub(super) const SINA_FUND_BATCH_REFERER: &str = "https://finance.sina.com.cn/";

pub(super) const SINA_FUND_BATCH_SIZE: usize = 750;

pub(super) const SINA_FUND_HISTORY_HOSTS: &[&str] = &["https://stock.finance.sina.com.cn"];

const FUND_NAV_HISTORY_PATH: &str = "/fundInfo/api/openapi.php/CaihuiFundInfoService.getNav";

const FUND_NAV_HISTORY_NUM: &str = "10000";

#[derive(Debug, Clone, PartialEq)]
pub struct SinaFundNavRow {
    pub code: String,
    pub name: String,
    pub nav_date: String,
    pub form: SinaFundNavForm,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SinaFundNavForm {
    UnitNav {
        unit_nav: f64,
        accumulated_nav: Option<f64>,
        prev_unit_nav: Option<f64>,
    },
    MoneyYield {
        gain_per_wan: Option<f64>,
        seven_day_annual_percent: Option<f64>,
    },
}

impl SinaFundNavRow {
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

pub(super) fn parse_sina_fund_nav_rows(
    body: &str,
) -> std::result::Result<Vec<SinaFundNavRow>, String> {
    let mut rows = Vec::new();
    let mut saw_statement = false;
    for (code, value) in fund_statements(body) {
        saw_statement = true;
        if let Some(row) = row_from_value(code, value) {
            rows.push(row);
        }
    }
    if !saw_statement {
        return Err("响应不含任何基金净值语句（疑似被拦截）".to_string());
    }
    Ok(rows)
}

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

fn raw_f64(field: &str) -> Option<f64> {
    field.trim().parse::<f64>().ok()
}

fn positive_f64(field: &str) -> Option<f64> {
    raw_f64(field).filter(|value| *value > 0.0)
}

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

pub(super) async fn fetch_sina_fund_nav_rows(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    codes: &[String],
) -> Result<Vec<SinaFundNavRow>> {
    let mut rows = Vec::new();
    for chunk in codes.chunks(SINA_FUND_BATCH_SIZE) {
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
    source_tail::finish(
        SOURCE_BATCH,
        &bytes,
        pacer,
        |response| decode_gbk(response).map_err(|error| error.to_string()),
        parse_sina_fund_nav_rows,
        malformed_batch_source,
    )
}

fn malformed_batch_source() -> AppError {
    AppError::coded(
        "sync.fund-batch-source-malformed",
        "新浪场外基金批量净值数据源返回了无法解析的内容，请稍后重试同步",
    )
}

#[derive(Debug, Deserialize)]
struct NavHistoryRow {
    #[serde(default, deserialize_with = "deserialize_flexible_string")]
    fbrq: Option<String>,
    #[serde(default, deserialize_with = "deserialize_flexible_string")]
    jjjz: Option<String>,
}

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

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
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

pub(super) fn parse_fund_nav_history(body: &str) -> std::result::Result<Vec<NavPoint>, String> {
    let resp: NavHistoryResponse =
        serde_json::from_str(body).map_err(|error| format!("不是可信 JSON 报文：{error}"))?;
    parse_fund_nav_history_response(resp)
}

fn parse_fund_nav_history_response(
    resp: NavHistoryResponse,
) -> std::result::Result<Vec<NavPoint>, String> {
    let result = resp
        .result
        .ok_or_else(|| "缺 result（非 JSON 或被拦截）".to_string())?;
    if result
        .status
        .and_then(|status| status.code)
        .is_some_and(|code| code != "0")
    {
        return Err("自报失败（status.code 非 0）".to_string());
    }
    let data = result
        .data
        .ok_or_else(|| "缺 data（不可信形状）".to_string())?;
    let rows = data
        .rows
        .ok_or_else(|| "缺 data.data 数组（不可信形状）".to_string())?;
    let points: Vec<NavPoint> = rows
        .iter()
        .filter_map(|row| {
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
        return Err(format!(
            "有 {} 行但全部未通过解析纪律（不可信形状）",
            rows.len()
        ));
    }
    if let Some(total) = data
        .total_num
        .as_deref()
        .and_then(|t| t.trim().parse::<usize>().ok())
        && rows.len() < total
    {
        return Err(format!(
            "声明总数 {total} 未取全（实收 {} 行），窗口不完整",
            rows.len()
        ));
    }
    Ok(points)
}

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
    source_tail::finish(
        SOURCE_HISTORY,
        body.as_bytes(),
        pacer,
        source_tail::utf8,
        parse_fund_nav_history,
        unexpected_history_response,
    )
}

fn unexpected_history_response() -> AppError {
    AppError::Parse("新浪基金历史净值响应不可解析".into())
}
