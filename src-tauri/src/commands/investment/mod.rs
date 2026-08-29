mod crud;
mod holdings;
pub(crate) mod predicates;
mod reports;
#[cfg(test)]
mod tests;
mod trade;
mod trend;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{
    ExchangeRate, ExchangeRateInput, Holding, InstrumentInput, InstrumentListFilter,
    InstrumentListResult, InstrumentPriceTrend, MarketPrice, MarketPriceInput, PnlFilter,
    PortfolioValueTrend, RealizedPnlSummary, TransactionTrade, TrendRange,
};

pub(crate) use reports::query_realized_pnl_summary;
// 投资交易对外出口收窄为 prepare/apply/revert 三件套（issue #72 / spec #69）：
// 校验归一化（prepare）、应用副作用（apply）、回退副作用（revert）各一个入口，
// 不再暴露 create/update/cleanup/reverse 等散落函数；行写入经交易行为层编排。
pub(crate) use trade::{Plan, apply, prepare, revert};

#[tauri::command]
pub fn list_holdings(db: tauri::State<'_, DbState>) -> Result<Vec<Holding>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::list_holdings(&conn)
}

#[tauri::command]
pub fn instrument_price_trend(
    db: tauri::State<'_, DbState>,
    instrument_id: String,
    filter: Option<TrendRange>,
) -> Result<InstrumentPriceTrend> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    trend::query_instrument_price_trend(&conn, &instrument_id, &filter.unwrap_or_default())
}

#[tauri::command]
pub fn portfolio_value_trend(
    db: tauri::State<'_, DbState>,
    filter: Option<TrendRange>,
) -> Result<PortfolioValueTrend> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    portfolio_value_trend_internal(&conn, &filter.unwrap_or_default())
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行组合走势查询（先例：
/// [`list_instruments_internal`]，供 BDD 步骤复用与 IPC 命令同一实现）。
pub fn portfolio_value_trend_internal(
    conn: &rusqlite::Connection,
    filter: &TrendRange,
) -> Result<PortfolioValueTrend> {
    trend::query_portfolio_value_trend(conn, filter)
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

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行汇率写入（先例：
/// [`list_instruments_internal`]，供 BDD 步骤复用同一实现）。
pub fn create_exchange_rate_internal(
    conn: &rusqlite::Connection,
    input: ExchangeRateInput,
) -> Result<String> {
    crud::create_exchange_rate(conn, input)
}

#[tauri::command]
pub fn create_exchange_rate(
    db: tauri::State<'_, DbState>,
    input: ExchangeRateInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| create_exchange_rate_internal(conn, input))
}

#[tauri::command]
pub fn list_market_prices(db: tauri::State<'_, DbState>) -> Result<Vec<MarketPrice>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::list_market_prices(&conn)
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行行情写入（先例：
/// [`list_instruments_internal`]，供 BDD 步骤复用同一实现）。
pub fn create_market_price_internal(
    conn: &rusqlite::Connection,
    input: MarketPriceInput,
) -> Result<String> {
    crud::create_market_price(conn, input)
}

#[tauri::command]
pub fn create_market_price(
    db: tauri::State<'_, DbState>,
    input: MarketPriceInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| create_market_price_internal(conn, input))
}

#[tauri::command]
pub fn list_instruments(
    db: tauri::State<'_, DbState>,
    filter: Option<InstrumentListFilter>,
) -> Result<InstrumentListResult> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let filter = filter.unwrap_or_default();
    crud::list_instruments(&conn, &filter)
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行标的列表查询
/// （先例：`search::search_transactions_internal`，供 BDD 步骤复用同一实现）。
pub fn list_instruments_internal(
    conn: &rusqlite::Connection,
    filter: &InstrumentListFilter,
) -> Result<InstrumentListResult> {
    crud::list_instruments(conn, filter)
}

#[tauri::command]
pub fn get_transaction_trade(
    db: tauri::State<'_, DbState>,
    id: String,
) -> Result<TransactionTrade> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    trade::get_transaction_trade(&conn, &id)
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行新建标的（先例：
/// [`list_instruments_internal`]，供 BDD 步骤复用同一实现）。
pub fn create_instrument_internal(
    conn: &rusqlite::Connection,
    input: InstrumentInput,
) -> Result<String> {
    crud::create_instrument(conn, input)
}

#[tauri::command]
pub fn create_instrument(db: tauri::State<'_, DbState>, input: InstrumentInput) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏（含同名标的信息更新的 upsert 分支）。
    db.write(|conn| create_instrument_internal(conn, input))
}
