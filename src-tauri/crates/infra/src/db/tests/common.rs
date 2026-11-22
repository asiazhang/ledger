use std::sync::{Arc, Mutex};

use rusqlite::{Connection, params};

use crate::db::DbState;

pub(super) fn write_test_state() -> DbState {
    crate::db::register_after_commit_hook(ledger_backup::after_commit_hook);
    let conn = Arc::new(Mutex::new(tauri_app_lib::test_support::open()));
    DbState {
        read_conn: conn.clone(),
        conn,
    }
}

pub(super) fn dirty_state(state: &DbState) -> ledger_backup::AutoBackupState {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    ledger_backup::get_state(&conn).expect("读调度状态")
}

pub(super) fn probe_exchange_rate(
    conn: &Connection,
    id: &str,
    base: &str,
    quote: &str,
    rate: f64,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,'2026-01-01','manual',?5,1,'test')",
        params![id, base, quote, rate, tauri_app_lib::test_support::FIXED_NOW],
    )
}

pub(super) fn probe_price_history(
    conn: &Connection,
    id: &str,
    instrument_id: &str,
    trade_date: &str,
    price_cents: i64,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,'CNY','eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        params![id, instrument_id, trade_date, price_cents],
    )
}

pub(super) fn probe_fx_rate_history(
    conn: &Connection,
    id: &str,
    base: &str,
    quote: &str,
    trade_date: &str,
    rate: f64,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        params![id, base, quote, trade_date, rate],
    )
}

pub(super) fn probe_instrument_market(
    conn: &Connection,
    id: &str,
    symbol: &str,
    market: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'stock',NULL,'USD',?3,'2026-09-01T00:00:00Z','2026-09-01T00:00:00Z',1,'test')",
        params![id, symbol, market],
    )
}

pub(super) fn probe_instrument_source(
    conn: &Connection,
    id: &str,
    symbol: &str,
    market: &str,
    source: Option<&str>,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
         VALUES (?1,?2,'stock','万科A','CNY',?3,'2026-01-02T00:00:00Z','2026-01-02T00:00:00Z',1,'test',?4)",
        params![id, symbol, market, source],
    )
}
