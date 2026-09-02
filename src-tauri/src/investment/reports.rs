use rusqlite::Connection;

use crate::db::query::query_all;
use crate::error::Result;
use crate::models::{AccountPnl, InstrumentPnl, PnlDetail, PnlFilter, RealizedPnlSummary, YearPnl};

pub fn query_realized_pnl_summary(
    conn: &Connection,
    filter: &PnlFilter,
) -> Result<RealizedPnlSummary> {
    let base_from = "FROM security_lot_sales sls \
                     JOIN transactions t ON t.id = sls.sell_transaction_id \
                     JOIN security_transactions st ON st.transaction_id = sls.sell_transaction_id \
                     JOIN instruments i ON i.id = st.instrument_id \
                     JOIN accounts a ON a.id = t.account_id";

    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(acct_id) = &filter.account_id {
        params.push(Box::new(acct_id.clone()));
        conditions.push(format!("t.account_id=?{}", params.len()));
    }
    if let Some(inst_id) = &filter.instrument_id {
        params.push(Box::new(inst_id.clone()));
        conditions.push(format!("st.instrument_id=?{}", params.len()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let total_sql = format!(
        "SELECT COALESCE(SUM(sls.realized_pnl_cents), 0) {}{}",
        base_from, where_clause
    );
    let year_sql = format!(
        "SELECT substr(t.date, 1, 4) AS year, SUM(sls.realized_pnl_cents) \
         {}{} GROUP BY year ORDER BY year",
        base_from, where_clause
    );
    let account_sql = format!(
        "SELECT a.id, a.name, COALESCE(SUM(sls.realized_pnl_cents), 0) \
         {}{} GROUP BY a.id ORDER BY a.name",
        base_from, where_clause
    );
    let instrument_sql = format!(
        "SELECT i.id, i.symbol, i.name, COALESCE(SUM(sls.realized_pnl_cents), 0) \
         {}{} GROUP BY i.id ORDER BY i.symbol",
        base_from, where_clause
    );
    let detail_sql = format!(
        "SELECT sls.id, t.date, t.account_id, a.name, i.id, i.symbol, i.name, \
         sls.quantity, sls.cost_per_unit_cents, sls.realized_pnl_cents, sls.currency_code \
         {}{} ORDER BY t.date DESC, sls.created_at DESC",
        base_from, where_clause
    );

    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

    let total_realized_pnl_cents: i64 = conn
        .query_row(&total_sql, params_ref.as_slice(), |r| {
            r.get::<_, Option<i64>>(0)
        })
        .unwrap_or(None)
        .unwrap_or(0);

    let by_year: Vec<YearPnl> = query_all(conn, &year_sql, params_ref.as_slice())?;
    let by_account: Vec<AccountPnl> = query_all(conn, &account_sql, params_ref.as_slice())?;
    let by_instrument: Vec<InstrumentPnl> =
        query_all(conn, &instrument_sql, params_ref.as_slice())?;
    let details: Vec<PnlDetail> = query_all(conn, &detail_sql, params_ref.as_slice())?;

    Ok(RealizedPnlSummary {
        total_realized_pnl_cents,
        by_year,
        by_account,
        by_instrument,
        details,
    })
}
