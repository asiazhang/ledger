use serde::Deserialize;

use ledger_infra::error::{AppError, Result};
use ledger_investment::Quote;
use ledger_investment::prices::{CSRC_PRICE_SOURCE, SINA_PRICE_SOURCE, price_value_to_cents};

use super::csrc::{
    CSRC_HOSTS, confirm_money_fund_form_from, disclosure_window_dates, fetch_fund_nav_series_from,
};
use super::http::{ForegroundGuard, Pacer, build_client, lock_pacer, shared_pacer};
use super::sina_fund::{SINA_FUND_BATCH_HOSTS, SinaFundNavForm, fetch_sina_fund_nav_rows};

pub(super) fn deserialize_flexible_string<'de, D>(
    d: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

pub(super) async fn fetch_fund_quote(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    code: &str,
) -> Result<Quote> {
    fetch_fund_quote_from(client, pacer, code, SINA_FUND_BATCH_HOSTS, CSRC_HOSTS).await
}

pub(super) async fn fetch_fund_quote_from(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    code: &str,
    batch_hosts: &[&str],
    csrc_hosts: &[&str],
) -> Result<Quote> {
    tracing::debug!(code, "基金行情查询");
    let rows = fetch_sina_fund_nav_rows(client, pacer, batch_hosts, &[code.to_string()]).await?;
    if let Some(row) = rows.into_iter().find(|row| row.code == code) {
        if let SinaFundNavForm::UnitNav { unit_nav, .. } = row.form {
            return Ok(unit_nav_quote(
                code,
                &row.name,
                &row.nav_date,
                unit_nav,
                SINA_PRICE_SOURCE,
            ));
        }
        match confirm_money_fund_form_from(client, pacer, code, csrc_hosts).await {
            Ok(true) => return Ok(constant_price_quote(code, &row.name, &row.nav_date)),
            Ok(false) => {}
            Err(error) => return Err(error),
        }
    }
    let (start, end) = disclosure_window_dates();
    let records = fetch_fund_nav_series_from(client, pacer, code, &start, &end, csrc_hosts).await?;
    let latest = records
        .into_iter()
        .max_by(|a, b| a.valuation_date.cmp(&b.valuation_date));
    let Some(latest) = latest else {
        return Err(fund_not_found(code));
    };
    if latest.is_money_fund_form() {
        return Ok(constant_price_quote(
            code,
            &latest.name,
            &latest.valuation_date,
        ));
    }
    match latest.unit_nav {
        Some(nav) => Ok(unit_nav_quote(
            code,
            &latest.name,
            &latest.valuation_date,
            nav,
            CSRC_PRICE_SOURCE,
        )),
        None => Ok(name_only_quote(code, &latest.name)),
    }
}

fn unit_nav_quote(code: &str, name: &str, nav_date: &str, nav: f64, source: &'static str) -> Quote {
    Quote {
        code: code.to_string(),
        name: name.trim().to_string(),
        price_cents: Some(price_value_to_cents(nav)),
        price_date: Some(nav_date.to_string()),
        market: None,
        kind_hint: None,
        fund_class: None,
        nav_date: Some(nav_date.to_string()),
        constant_unit_price_cents: None,
        price_source: source,
    }
}

fn constant_price_quote(code: &str, name: &str, date: &str) -> Quote {
    let cents = price_value_to_cents(super::fund_nav::MONEY_FUND_UNIT_NAV);
    Quote {
        code: code.to_string(),
        name: name.trim().to_string(),
        price_cents: Some(cents),
        price_date: Some(date.to_string()),
        market: None,
        kind_hint: None,
        fund_class: None,
        nav_date: Some(date.to_string()),
        constant_unit_price_cents: Some(cents),
        price_source: CSRC_PRICE_SOURCE,
    }
}

fn name_only_quote(code: &str, name: &str) -> Quote {
    Quote {
        code: code.to_string(),
        name: name.trim().to_string(),
        price_cents: None,
        price_date: None,
        market: None,
        kind_hint: None,
        fund_class: None,
        nav_date: None,
        constant_unit_price_cents: None,
        price_source: CSRC_PRICE_SOURCE,
    }
}

const FUND_NOT_FOUND_CODE: &str = "sync.fund-not-found";

fn fund_not_found(code: &str) -> AppError {
    AppError::codedp(
        FUND_NOT_FOUND_CODE,
        format!("查无基金代码 {code}，请核对后重试"),
        &[code],
    )
}

pub(super) fn is_fund_not_found(error: &AppError) -> bool {
    error.is_code(FUND_NOT_FOUND_CODE)
}

pub async fn fetch_fund_quote_production(code: &str) -> Result<Quote> {
    let _foreground = ForegroundGuard::enter();
    let client = build_client()?;
    let pacer = shared_pacer();
    let mut pacer = lock_pacer(&pacer).await;
    fetch_fund_quote(&client, &mut pacer, code).await
}
