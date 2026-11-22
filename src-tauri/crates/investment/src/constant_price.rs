use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, params};

use super::prices::{MarketPriceWrite, upsert_market_price};
use ledger_infra::db::now_iso;
use ledger_infra::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantPriceValue {
    pub instrument_id: String,
    pub price_cents: i64,
    pub currency_code: String,
    pub anchor_date: NaiveDate,
}

pub fn mark_constant_unit_price(
    conn: &Connection,
    instrument_id: &str,
    constant_unit_price_cents: i64,
) -> Result<bool> {
    let now = now_iso();
    let marked = conn.execute(
        "UPDATE instruments SET constant_unit_price = ?2, updated_at = ?3 \
         WHERE id = ?1 AND constant_unit_price IS NULL",
        params![instrument_id, constant_unit_price_cents, now],
    )? > 0;
    if !marked {
        return Ok(false);
    }
    conn.execute(
        "UPDATE market_prices SET nav_date = NULL, updated_at = ?2, version = version + 1 \
         WHERE instrument_id = ?1 AND nav_date IS NOT NULL",
        params![instrument_id, now],
    )?;
    Ok(true)
}

pub fn ensure_constant_base_price(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    currency_code: &str,
    priced_at: &str,
    source: &str,
) -> Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM market_prices WHERE instrument_id = ?1)",
        [instrument_id],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(false);
    }
    upsert_market_price(
        conn,
        &MarketPriceWrite {
            instrument_id,
            price_cents,
            currency_code,
            priced_at,
            nav_date: None,
            source: Some(source),
        },
    )?;
    Ok(true)
}

pub fn load_constant_prices(conn: &Connection) -> Result<Vec<ConstantPriceValue>> {
    let mut stmt = conn.prepare(
        "SELECT id, constant_unit_price, currency_code, created_at FROM instruments \
         WHERE constant_unit_price IS NOT NULL ORDER BY id",
    )?;
    let rows = stmt.query_map([], map_constant_value)?;
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

fn map_constant_value(row: &rusqlite::Row) -> rusqlite::Result<ConstantPriceValue> {
    let created_at: String = row.get(3)?;
    let anchor_date = created_at
        .get(..10)
        .and_then(|head| NaiveDate::parse_from_str(head, "%Y-%m-%d").ok())
        .unwrap_or_else(crate::staleness::beijing_today);
    Ok(ConstantPriceValue {
        instrument_id: row.get(0)?,
        price_cents: row.get(1)?,
        currency_code: row.get(2)?,
        anchor_date,
    })
}

pub fn week_monday(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

pub fn weekly_samples(from: NaiveDate, to: NaiveDate, today: NaiveDate) -> Vec<(String, String)> {
    let end = week_monday(to).min(week_monday(today));
    let mut cursor = week_monday(from);
    let mut samples = Vec::new();
    while cursor <= end {
        let trade_date = (cursor + chrono::Duration::days(6)).min(to).min(today);
        samples.push((
            cursor.format("%Y-%m-%d").to_string(),
            trade_date.format("%Y-%m-%d").to_string(),
        ));
        cursor += chrono::Duration::days(7);
    }
    samples
}
