use rusqlite::{Connection, params};

use super::FIXED_NOW;

pub fn seed_account(
    conn: &Connection,
    id: &str,
    name: &str,
    kind: &str,
    currency: &str,
    initial_balance_cents: i64,
) -> String {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?6,1,'test',0)",
        params![id, name, kind, currency, initial_balance_cents, FIXED_NOW],
    )
    .unwrap();
    id.to_string()
}

pub fn seed_instrument(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
) -> String {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'stock',?3,?4,?5,?6,?6,1,'test')",
        params![id, symbol, name, currency, market, FIXED_NOW],
    )
    .unwrap();
    id.to_string()
}

pub fn seed_exchange_rate(conn: &Connection, base: &str, quote: &str, rate: f64) -> String {
    seed_exchange_rate_row(conn, base, quote, rate, FIXED_NOW, None)
}

pub fn seed_exchange_rate_with_source(
    conn: &Connection,
    base: &str,
    quote: &str,
    rate: f64,
    priced_at: &str,
    source: &str,
) -> String {
    seed_exchange_rate_row(conn, base, quote, rate, priced_at, Some(source))
}

fn seed_exchange_rate_row(
    conn: &Connection,
    base: &str,
    quote: &str,
    rate: f64,
    priced_at: &str,
    source: Option<&str>,
) -> String {
    let id = format!("er-{base}-{quote}");
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,1,'test')",
        params![id, base, quote, rate, priced_at, source, FIXED_NOW],
    )
    .unwrap();
    id
}

pub fn seed_market_price(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    currency_code: &str,
) -> String {
    seed_market_price_row(
        conn,
        instrument_id,
        price_cents,
        currency_code,
        FIXED_NOW,
        None,
        None,
    )
}

pub fn seed_fund_market_price(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    currency_code: &str,
    nav_date: &str,
    source: Option<&str>,
) -> String {
    seed_market_price_row(
        conn,
        instrument_id,
        price_cents,
        currency_code,
        nav_date,
        Some(nav_date),
        source,
    )
}

fn seed_market_price_row(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    currency_code: &str,
    priced_at: &str,
    nav_date: Option<&str>,
    source: Option<&str>,
) -> String {
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,nav_date,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8,1,'test')",
        params![
            ledger_infra::db::new_uuid(),
            instrument_id,
            price_cents,
            currency_code,
            priced_at,
            nav_date,
            source,
            FIXED_NOW,
        ],
    )
    .unwrap();
    instrument_id.to_string()
}

pub fn seed_price_history(
    conn: &Connection,
    id: &str,
    instrument_id: &str,
    trade_date: &str,
    price_cents: i64,
    currency: &str,
) -> String {
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,'test')",
        params![id, instrument_id, trade_date, price_cents, currency, FIXED_NOW],
    )
    .unwrap();
    id.to_string()
}

pub fn seed_fx_rate_history(
    conn: &Connection,
    id: &str,
    base: &str,
    quote: &str,
    trade_date: &str,
    rate: f64,
) -> String {
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,'test')",
        params![id, base, quote, trade_date, rate, FIXED_NOW],
    )
    .unwrap();
    id.to_string()
}

pub fn seed_investment_setup(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
) -> (String, String) {
    seed_account(conn, account_id, "美股", "investment", "USD", 0);
    seed_instrument(conn, instrument_id, "SYM", "Symbol", "USD", "unknown");
    seed_exchange_rate(conn, "USD", "CNY", 1.0);
    seed_fx_history_series_1to1(conn, "USD", "CNY");
    (account_id.to_string(), instrument_id.to_string())
}

pub fn seed_fx_history_weeks(
    conn: &Connection,
    base: &str,
    quote: &str,
    rate: f64,
    dates: &[&str],
) {
    let mut done: Vec<String> = Vec::new();
    for d in dates {
        let week: String = conn
            .query_row("SELECT date(?1,'-6 days','weekday 1')", params![d], |r| {
                r.get(0)
            })
            .unwrap();
        if done.contains(&week) {
            continue;
        }
        done.push(week.clone());
        conn.execute(
            "INSERT OR IGNORE INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,?6,'eastmoney',?5,?5,1,'test')",
            params![format!("fxh-{base}-{quote}-{week}"), base, quote, week, FIXED_NOW, rate],
        )
        .unwrap();
    }
}

fn seed_fx_history_series_1to1(conn: &Connection, base: &str, quote: &str) {
    let mut week = "2025-01-06".to_string();
    let mut dates: Vec<String> = Vec::with_capacity(160);
    for _ in 0..160 {
        dates.push(week.clone());
        week = conn
            .query_row("SELECT date(?1,'+7 days')", params![week], |r| {
                r.get::<_, String>(0)
            })
            .unwrap();
    }
    let refs: Vec<&str> = dates.iter().map(String::as_str).collect();
    seed_fx_history_weeks(conn, base, quote, 1.0, &refs);
}
