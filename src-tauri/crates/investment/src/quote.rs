use rusqlite::Connection;

use super::constant_price::{ensure_constant_base_price, mark_constant_unit_price};
use super::crud;
use super::market::Market;
use super::model::{InstrumentInput, InstrumentType};
use super::prices::{MarketPriceWrite, upsert_market_price};
use ledger_infra::error::{AppError, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub code: String,
    pub name: String,
    pub price_cents: Option<i64>,
    pub price_date: Option<String>,
    pub market: Option<Market>,
    pub kind_hint: Option<InstrumentType>,
    pub fund_class: Option<String>,
    pub nav_date: Option<String>,
    pub constant_unit_price_cents: Option<i64>,
    pub price_source: &'static str,
}

impl Quote {
    pub fn stock_market(&self) -> Result<Market> {
        self.market
            .ok_or_else(|| AppError::coded("quote.market-missing", "行情缺少市场（内部不一致）"))
    }

    pub fn stock_kind_hint(&self) -> InstrumentType {
        self.kind_hint.unwrap_or(InstrumentType::Stock)
    }
}

pub struct QuoteAdoptionInput<'a> {
    pub kind: InstrumentType,
    pub market: Market,
    pub currency_code: &'a str,
    pub priced_at: &'a str,
    pub nav_date: Option<&'a str>,
}

pub struct QuoteAdoptionOutcome {
    pub instrument_id: String,
    pub price_written: bool,
}

pub fn adopt_quote(
    conn: &Connection,
    adoption: &QuoteAdoptionInput<'_>,
    quote: &Quote,
) -> Result<QuoteAdoptionOutcome> {
    let instrument_id = crud::create_instrument(
        conn,
        InstrumentInput {
            symbol: quote.code.clone(),
            kind: adoption.kind,
            name: Some(quote.name.clone()),
            currency_code: adoption.currency_code.to_string(),
            market: Some(adoption.market.as_str().to_string()),
        },
    )?;
    let mut price_written = false;
    if let Some(constant_unit_price_cents) = quote.constant_unit_price_cents {
        mark_constant_unit_price(conn, &instrument_id, constant_unit_price_cents)?;
        let price_cents = quote.price_cents.unwrap_or(constant_unit_price_cents);
        price_written = ensure_constant_base_price(
            conn,
            &instrument_id,
            price_cents,
            adoption.currency_code,
            adoption.priced_at,
            quote.price_source,
        )?;
        return Ok(QuoteAdoptionOutcome {
            instrument_id,
            price_written,
        });
    }
    if let Some(price_cents) = quote.price_cents {
        upsert_market_price(
            conn,
            &MarketPriceWrite {
                instrument_id: &instrument_id,
                price_cents,
                currency_code: adoption.currency_code,
                priced_at: adoption.priced_at,
                nav_date: adoption.nav_date,
                source: Some(quote.price_source),
            },
        )?;
        price_written = true;
    }
    Ok(QuoteAdoptionOutcome {
        instrument_id,
        price_written,
    })
}
