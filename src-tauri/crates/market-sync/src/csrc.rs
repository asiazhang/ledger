use serde::Deserialize;

use ledger_infra::error::{AppError, Result};

use super::fund::deserialize_flexible_string;
use super::http::{Pacer, RetryConfig, request_text_from_hosts};
use super::source_tail;

const SOURCE: &str = "csrc-disclosure";

pub(super) const CSRC_HOSTS: &[&str] = &["http://eid.csrc.gov.cn"];

const QUERY_PATH: &str = "/fund/disclose/getPublicFundJZInfoMore.do";

const PAGE_SIZE: i64 = 500;

const MAX_PAGES: usize = 8;

fn malformed_source() -> AppError {
    AppError::coded(
        "sync.disclosure-source-malformed",
        "基金官方披露数据源返回了无法解析的内容，请稍后重试同步",
    )
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct DisclosurePage {
    pub(super) records: Vec<CsrcNavRecord>,
    pub(super) total: i64,
    pub(super) raw: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CsrcNavRecord {
    pub code: String,
    pub name: String,
    pub valuation_date: String,
    pub unit_nav: Option<f64>,
    pub accumulated_nav: Option<f64>,
    pub gain_per: Option<f64>,
    pub seven_day_yield_percent: Option<f64>,
}

impl CsrcNavRecord {
    pub fn is_money_fund_form(&self) -> bool {
        self.unit_nav.is_none() && self.gain_per.is_some() && self.seven_day_yield_percent.is_some()
    }
}

#[derive(Debug, Deserialize)]
struct DisclosureResponse {
    #[serde(default)]
    success: Option<bool>,
    #[serde(rename = "iTotalRecords", default)]
    total: i64,
    #[serde(rename = "aaData", default)]
    rows: Option<Vec<DisclosureRow>>,
}

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
    serde_json::to_string(&ao).map_err(|error| AppError::Io(format!("aoData 序列化失败: {error}")))
}

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

fn raw_value(raw: Option<&str>) -> Option<f64> {
    let raw = raw?.trim();
    let raw = raw.strip_suffix('%').unwrap_or(raw).trim();
    raw.parse::<f64>().ok()
}

fn positive_value(raw: Option<&str>) -> Option<f64> {
    raw_value(raw).filter(|value| *value > 0.0)
}

const DISCLOSURE_WINDOW_MONTHS: chrono::Months = chrono::Months::new(120);

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
