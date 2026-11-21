use rusqlite::{Connection, params};

use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::Result;
use ledger_sync_protocol::device::device_id;

pub const PRICE_UNITS_PER_FEN: f64 = 100.0;

pub const EASTMONEY_PRICE_SOURCE: &str = "eastmoney";

pub const TENCENT_PRICE_SOURCE: &str = "tencent";

pub const SINA_PRICE_SOURCE: &str = "sina";

pub const CSRC_PRICE_SOURCE: &str = "csrc";

pub fn price_value_to_cents(value: f64) -> i64 {
    (value * 10000.0).round() as i64
}

pub fn price_cents_to_value(cents: i64) -> f64 {
    cents as f64 / 10000.0
}

pub fn upsert_price_history(
    conn: &Connection,
    instrument_id: &str,
    trade_date: &str,
    price_cents: i64,
    currency: &str,
    source: &str,
) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7,1,?8) \
         ON CONFLICT(instrument_id, week_start) DO UPDATE SET \
         trade_date=excluded.trade_date, price_cents=excluded.price_cents, \
         currency_code=excluded.currency_code, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![new_uuid(), instrument_id, trade_date, price_cents, currency, source, now, device_id(conn)?],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct MarketPriceWrite<'a> {
    pub instrument_id: &'a str,
    pub price_cents: i64,
    pub currency_code: &'a str,
    pub priced_at: &'a str,
    pub nav_date: Option<&'a str>,
    pub source: Option<&'a str>,
}

pub fn upsert_market_price(conn: &Connection, input: &MarketPriceWrite<'_>) -> Result<String> {
    let MarketPriceWrite {
        instrument_id,
        price_cents,
        currency_code,
        priced_at,
        nav_date,
        source,
    } = *input;
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM market_prices WHERE instrument_id=?1",
            params![instrument_id],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or_else(new_uuid);
    let now = now_iso();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,nav_date,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) \
         ON CONFLICT(instrument_id) DO UPDATE SET \
         price_cents=excluded.price_cents, currency_code=excluded.currency_code, \
         priced_at=excluded.priced_at, nav_date=excluded.nav_date, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![
            id,
            instrument_id,
            price_cents,
            currency_code,
            priced_at,
            nav_date,
            source,
            now,
            now,
            1,
            device_id(conn)?
        ],
    )?;
    Ok(id)
}
