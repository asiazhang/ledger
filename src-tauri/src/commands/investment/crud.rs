use rusqlite::Connection;

use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{
    ExchangeRate, ExchangeRateInput, Holding, InstrumentInput, InstrumentListFilter,
    InstrumentListResult, MarketPrice, MarketPriceInput,
};

pub(crate) fn list_holdings(conn: &Connection) -> Result<Vec<Holding>> {
    query_all(
        conn,
        "SELECT id,account_id,instrument_id,quantity,cost_basis_cents,cost_currency_code, \
         latest_price_cents,latest_price_currency_code,market_value_cents,unrealized_pnl_cents,updated_at \
         FROM v_holdings ORDER BY account_id, instrument_id",
        [],
    )
}

pub(crate) fn list_exchange_rates(conn: &Connection) -> Result<Vec<ExchangeRate>> {
    query_all(
        conn,
        "SELECT id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id \
         FROM exchange_rates ORDER BY base_code, quote_code",
        [],
    )
}

pub(crate) fn create_exchange_rate(conn: &Connection, input: ExchangeRateInput) -> Result<String> {
    if input.rate <= 0.0 {
        return Err(AppError::Invalid("汇率必须大于 0".into()));
    }
    let id = new_uuid();
    let now = now_iso();
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            rusqlite::params![input.base_code, input.quote_code],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or(id);
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
         ON CONFLICT(base_code, quote_code) DO UPDATE SET \
         rate=excluded.rate, priced_at=excluded.priced_at, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1, device_id=excluded.device_id",
        rusqlite::params![
            id,
            input.base_code,
            input.quote_code,
            input.rate,
            input.priced_at,
            input.source,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

pub(crate) fn list_market_prices(conn: &Connection) -> Result<Vec<MarketPrice>> {
    query_all(
        conn,
        "SELECT id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id \
         FROM market_prices ORDER BY instrument_id, priced_at DESC",
        [],
    )
}

pub(crate) fn create_market_price(conn: &Connection, input: MarketPriceInput) -> Result<String> {
    if input.price_cents <= 0 {
        return Err(AppError::Invalid("价格必须大于 0".into()));
    }
    let id = new_uuid();
    let now = now_iso();
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM market_prices WHERE instrument_id=?1",
            rusqlite::params![input.instrument_id],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or(id);
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) \
         ON CONFLICT(instrument_id) DO UPDATE SET \
         price_cents=excluded.price_cents, currency_code=excluded.currency_code, \
         priced_at=excluded.priced_at, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1, device_id=excluded.device_id",
        rusqlite::params![
            id,
            input.instrument_id,
            input.price_cents,
            input.currency_code,
            input.priced_at,
            input.source,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

pub(crate) fn list_instruments(
    conn: &Connection,
    filter: &InstrumentListFilter,
) -> Result<InstrumentListResult> {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(search) = filter
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        params.push(Box::new(format!("%{}%", search.to_lowercase())));
        conditions.push(format!(
            "(LOWER(i.symbol) LIKE ?{} OR LOWER(COALESCE(i.name, '')) LIKE ?{})",
            params.len(),
            params.len()
        ));
    }
    if let Some(market) = filter.market.as_deref().filter(|m| !m.is_empty()) {
        params.push(Box::new(market.to_string()));
        conditions.push(format!("i.market=?{}", params.len()));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM instruments i{where_clause}"),
        params_ref.as_slice(),
        |r| r.get(0),
    )?;

    let page = filter.page.unwrap_or(1).max(1);
    let page_size = filter.page_size.unwrap_or(50).clamp(1, 500);
    let offset = (page - 1) * page_size;
    params.push(Box::new(page_size as i64));
    params.push(Box::new(offset as i64));

    let sql = format!(
        "SELECT i.id,i.symbol,i.instrument_type,i.name,i.currency_code,i.market,i.created_at,i.updated_at,i.version,i.device_id,p.price_cents \
         FROM instruments i \
         LEFT JOIN market_prices p ON p.instrument_id = i.id \
         {where_clause} ORDER BY i.symbol LIMIT ?{} OFFSET ?{}",
        params.len() - 1,
        params.len()
    );
    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let items = query_all(conn, &sql, params_ref.as_slice())?;

    Ok(InstrumentListResult { items, total })
}

pub(crate) fn create_instrument(conn: &Connection, input: InstrumentInput) -> Result<String> {
    if input.symbol.trim().is_empty() {
        return Err(AppError::Invalid("标的代码不能为空".into()));
    }
    let market = input.market.as_deref().unwrap_or("unknown");
    let existing_id: Option<(String, Option<String>, String)> = conn
        .query_row(
            "SELECT id, name, market FROM instruments WHERE symbol=?1 AND instrument_type=?2",
            rusqlite::params![input.symbol, input.kind],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    if let Some((existing_id, existing_name, existing_market)) = existing_id {
        let name_changed = input.name != existing_name;
        let market_changed = market != existing_market;
        if name_changed || market_changed {
            let now = now_iso();
            conn.execute(
                "UPDATE instruments SET name=?1, market=?2, updated_at=?3, version=version+1 WHERE id=?4",
                rusqlite::params![input.name, market, now, existing_id],
            )?;
        }
        return Ok(existing_id);
    }
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        rusqlite::params![
            id,
            input.symbol,
            input.kind,
            input.name,
            input.currency_code,
            market,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}
