use chrono::NaiveDate;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::channel::{PriceChannel, derive_price_channel};
use super::market::Market;
use super::model::InstrumentType;
use super::predicates::INVESTED_EXISTS;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::error::Result;

pub const PRICE_STALE_AFTER_DAYS: i64 = 7;

pub const PRICE_STALE_CLEARED_EXIT_DAYS: i64 = 365;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceStaleness {
    pub stale_count: usize,
    pub threshold_days: i64,
}

pub fn instrument_price_staleness(conn: &Connection) -> Result<PriceStaleness> {
    instrument_price_staleness_on(conn, beijing_today())
}

pub fn instrument_price_staleness_on(
    conn: &Connection,
    today: NaiveDate,
) -> Result<PriceStaleness> {
    let rows = query_all::<PriceWatermarkRow, _>(
        conn,
        &format!(
            "SELECT i.instrument_type,i.market,i.symbol,i.constant_unit_price,p.priced_at,p.nav_date, \
             CASE WHEN {INVESTED_EXISTS} THEN 1 ELSE 0 END AS invested \
             FROM instruments i \
             LEFT JOIN market_prices p ON p.instrument_id = i.id"
        ),
        [],
    )?;

    let stale_count = rows
        .iter()
        .filter(|row| row.is_stale(today, PRICE_STALE_AFTER_DAYS))
        .count();

    Ok(PriceStaleness {
        stale_count,
        threshold_days: PRICE_STALE_AFTER_DAYS,
    })
}

struct PriceWatermarkRow {
    kind: InstrumentType,
    market: Market,
    symbol: String,
    constant_unit_price: Option<i64>,
    priced_at: Option<String>,
    nav_date: Option<String>,
    invested: bool,
}

impl PriceWatermarkRow {
    fn is_stale(&self, today: NaiveDate, threshold_days: i64) -> bool {
        let channel = derive_price_channel(
            self.kind,
            self.market,
            &self.symbol,
            self.constant_unit_price,
        );
        let watermark = match channel {
            PriceChannel::Quote => self.priced_at.as_deref().and_then(quote_watermark_date),
            PriceChannel::FundNav => self.nav_date.as_deref().and_then(iso_date),
            PriceChannel::Constant | PriceChannel::Manual | PriceChannel::None => return false,
        };
        match watermark {
            Some(date) => {
                let days = today.signed_duration_since(date).num_days();
                if days <= threshold_days {
                    false
                } else if self.invested {
                    true
                } else {
                    days <= PRICE_STALE_CLEARED_EXIT_DAYS
                }
            }
            None => self.invested,
        }
    }
}

impl FromRow for PriceWatermarkRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PriceWatermarkRow {
            kind: row.get(0)?,
            market: row.get(1)?,
            symbol: row.get(2)?,
            constant_unit_price: row.get(3)?,
            priced_at: row.get(4)?,
            nav_date: row.get(5)?,
            invested: row.get::<_, i64>(6)? != 0,
        })
    }
}

fn quote_watermark_date(raw: &str) -> Option<NaiveDate> {
    let raw = raw.trim();
    match chrono::DateTime::parse_from_rfc3339(raw) {
        Ok(at) => Some(beijing_date(at.with_timezone(&chrono::Utc))),
        Err(_) => iso_date(raw),
    }
}

fn iso_date(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").ok()
}

pub(super) fn beijing_today() -> NaiveDate {
    beijing_date(chrono::Utc::now())
}

pub(super) fn beijing_date(now: chrono::DateTime<chrono::Utc>) -> NaiveDate {
    (now + chrono::Duration::hours(8)).date_naive()
}
