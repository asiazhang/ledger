use rusqlite::Connection;

use super::crud;
use super::market::Market;
use super::model::{AddFundResult, InstrumentInput, InstrumentType};
use super::quote::{Quote, QuoteAdoptionInput, adopt_quote};
use ledger_infra::error::{AppError, Result};

pub(crate) const FUND_MARKET: &str = "unknown";
const FUND_CURRENCY: &str = "CNY";

pub(crate) fn reject_non_unknown_fund_market(market: &str) -> Result<()> {
    if market == FUND_MARKET {
        return Ok(());
    }
    Err(AppError::codedp(
        "instrument.fund-market-forbidden",
        format!("基金标的市场恒为未知：不能携带市场 {market}（场外基金无交易所市场概念）"),
        &[market],
    ))
}

pub fn reject_carried_fund_market(market: Option<&str>) -> Result<()> {
    match market {
        Some(market) => reject_non_unknown_fund_market(market),
        None => Ok(()),
    }
}

pub fn is_six_digit_code(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

pub fn validate_fund_code(code: &str) -> Result<()> {
    if is_six_digit_code(code) {
        Ok(())
    } else {
        Err(AppError::coded(
            "fund.code-invalid",
            "基金代码须为 6 位数字",
        ))
    }
}

pub struct FundCreateOutcome {
    pub instrument_id: String,
    pub price_written: bool,
}

pub fn create_fund_degraded(
    conn: &Connection,
    symbol: &str,
    ai_name: Option<String>,
) -> Result<FundCreateOutcome> {
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1 AND instrument_type='fund'",
            rusqlite::params![symbol],
            |r| r.get(0),
        )
        .ok();
    if let Some(instrument_id) = existing_id {
        return Ok(FundCreateOutcome {
            instrument_id,
            price_written: false,
        });
    }
    let instrument_id = crud::create_instrument(
        conn,
        InstrumentInput {
            symbol: symbol.to_string(),
            kind: InstrumentType::Fund,
            name: ai_name,
            currency_code: FUND_CURRENCY.to_string(),
            market: Some(FUND_MARKET.to_string()),
        },
    )?;
    Ok(FundCreateOutcome {
        instrument_id,
        price_written: false,
    })
}

pub fn adopt_fund_quote(conn: &Connection, quote: &Quote) -> Result<AddFundResult> {
    let nav_date = quote.nav_date.as_deref();
    let outcome = adopt_quote(
        conn,
        &QuoteAdoptionInput {
            kind: InstrumentType::Fund,
            market: Market::Unknown,
            currency_code: FUND_CURRENCY,
            priced_at: nav_date.unwrap_or_default(),
            nav_date,
        },
        quote,
    )?;
    Ok(AddFundResult {
        instrument_id: outcome.instrument_id,
        symbol: quote.code.clone(),
        name: quote.name.clone(),
        fund_class: quote.fund_class.clone().unwrap_or_default(),
        nav_cents: quote.price_cents,
        nav_date: quote.nav_date.clone(),
        price_written: outcome.price_written,
    })
}

pub fn add_fund_by_code_with<F>(
    conn: &Connection,
    code: &str,
    fetch: &mut F,
) -> Result<AddFundResult>
where
    F: FnMut(&str, &str) -> Result<Quote>,
{
    validate_fund_code(code)?;
    let quote = fetch(code, FUND_MARKET)?;
    adopt_fund_quote(conn, &quote)
}
