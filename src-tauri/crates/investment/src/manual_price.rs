use chrono::NaiveDate;
use rusqlite::Connection;

use super::command::{PriceCommand, record_price};
use super::model::{MANUAL_SOURCE, ManualPriceInput, ManualPriceResult};
use super::prices::{MarketPriceWrite, upsert_market_price, upsert_price_history};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};

pub fn record_manual_price(
    conn: &Connection,
    input: &ManualPriceInput,
) -> Result<ManualPriceResult> {
    ensure_transaction(conn, || {
        let outcome = write_manual_price(conn, input)?;
        record_price(
            conn,
            PriceCommand::ManualPrice {
                instrument_id: input.instrument_id.clone(),
                date: input.date.clone(),
                price_cents: input.price_cents,
            },
        )?;
        Ok(outcome)
    })
}

pub(crate) fn write_manual_price(
    conn: &Connection,
    input: &ManualPriceInput,
) -> Result<ManualPriceResult> {
    if input.price_cents <= 0 {
        return Err(AppError::coded(
            "instrument.price-positive",
            "价格必须大于 0",
        ));
    }
    let trade_date = NaiveDate::parse_from_str(input.date.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::coded("instrument.price-date-format", "日期格式须为 YYYY-MM-DD"))?
        .format("%Y-%m-%d")
        .to_string();
    let (instrument_id, currency): (String, String) = conn
        .query_row(
            "SELECT id, currency_code FROM instruments WHERE id=?1",
            rusqlite::params![input.instrument_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| AppError::coded("manual-price.instrument-not-found", "标的不存在"))?;

    let latest_before: Option<String> = conn.query_row(
        "SELECT MAX(trade_date) FROM price_history WHERE instrument_id=?1",
        rusqlite::params![instrument_id],
        |r| r.get(0),
    )?;

    upsert_price_history(
        conn,
        &instrument_id,
        &trade_date,
        input.price_cents,
        &currency,
        MANUAL_SOURCE,
    )?;

    let current_price_written = latest_before
        .as_ref()
        .is_none_or(|latest| trade_date.as_str() >= latest.as_str());
    if current_price_written {
        upsert_market_price(
            conn,
            &MarketPriceWrite {
                instrument_id: &instrument_id,
                price_cents: input.price_cents,
                currency_code: &currency,
                priced_at: &trade_date,
                nav_date: None,
                source: Some(MANUAL_SOURCE),
            },
        )?;
    }

    Ok(ManualPriceResult {
        history_written: true,
        current_price_written,
    })
}
