mod crud;
mod reports;
#[cfg(test)]
mod tests;
mod trade;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{
    ExchangeRate, ExchangeRateInput, Holding, Instrument, InstrumentInput, MarketPrice,
    MarketPriceInput, PnlFilter, RealizedPnlSummary,
};

pub(crate) use reports::query_realized_pnl_summary;
pub(crate) use trade::{create_buy_transaction, create_sell_transaction};

#[tauri::command]
pub fn list_holdings(db: tauri::State<'_, DbState>) -> Result<Vec<Holding>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::list_holdings(&conn)
}

#[tauri::command]
pub fn realized_pnl_summary(
    db: tauri::State<'_, DbState>,
    filter: Option<PnlFilter>,
) -> Result<RealizedPnlSummary> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let filter = filter.unwrap_or(PnlFilter {
        account_id: None,
        instrument_id: None,
    });
    query_realized_pnl_summary(&conn, &filter)
}

#[tauri::command]
pub fn list_exchange_rates(db: tauri::State<'_, DbState>) -> Result<Vec<ExchangeRate>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::list_exchange_rates(&conn)
}

#[tauri::command]
pub fn create_exchange_rate(
    db: tauri::State<'_, DbState>,
    input: ExchangeRateInput,
) -> Result<String> {
    if input.rate <= 0.0 {
        return Err(AppError::Invalid("汇率必须大于 0".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::create_exchange_rate(&conn, input)
}

#[tauri::command]
pub fn list_market_prices(db: tauri::State<'_, DbState>) -> Result<Vec<MarketPrice>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::list_market_prices(&conn)
}

#[tauri::command]
pub fn create_market_price(
    db: tauri::State<'_, DbState>,
    input: MarketPriceInput,
) -> Result<String> {
    if input.price_cents <= 0 {
        return Err(AppError::Invalid("价格必须大于 0".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::create_market_price(&conn, input)
}

#[tauri::command]
pub fn list_instruments(db: tauri::State<'_, DbState>) -> Result<Vec<Instrument>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::list_instruments(&conn)
}

#[tauri::command]
pub fn create_instrument(db: tauri::State<'_, DbState>, input: InstrumentInput) -> Result<String> {
    if input.symbol.trim().is_empty() {
        return Err(AppError::Invalid("标的代码不能为空".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::create_instrument(&conn, input)
}
