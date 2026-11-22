use chrono::NaiveDate;
use rusqlite::{Connection, params};
use std::collections::HashMap;

use ledger_infra::error::{AppError, Result};

pub const FIRST_POSITION_DATE: &str = "(SELECT MIN(x) FROM (\
  SELECT MIN(t.date) AS x FROM security_transactions st INDEXED BY idx_security_transactions_instrument \
    JOIN transactions t ON t.id = st.transaction_id \
    JOIN accounts a ON a.id = t.account_id \
   WHERE t.is_deleted = 0 AND a.is_deleted = 0 \
    AND st.action IN ('buy','sell','convert','split') AND st.quantity IS NOT NULL \
    AND st.instrument_id = i.id \
  UNION ALL \
  SELECT MIN(t.date) FROM security_transactions st INDEXED BY idx_security_transactions_to_instrument \
    JOIN transactions t ON t.id = st.transaction_id \
    JOIN accounts a ON a.id = t.account_id \
   WHERE t.is_deleted = 0 AND a.is_deleted = 0 \
    AND st.action = 'convert' AND st.to_quantity IS NOT NULL \
    AND st.to_instrument_id = i.id))";

pub fn first_position_date(conn: &Connection, instrument_id: &str) -> Result<Option<String>> {
    let sql = format!("SELECT {FIRST_POSITION_DATE} FROM instruments i WHERE i.id = ?1");
    let date: Option<String> = conn
        .query_row(&sql, params![instrument_id], |r| r.get(0))
        .map_err(AppError::from)?;
    Ok(date)
}

pub fn holdings_as_of(
    conn: &Connection,
    instrument_id: Option<&str>,
    as_of_date: &str,
) -> Result<f64> {
    holdings_as_of_in(conn, None, instrument_id, as_of_date)
}

pub fn holdings_as_of_in(
    conn: &Connection,
    account_id: Option<&str>,
    instrument_id: Option<&str>,
    as_of_date: &str,
) -> Result<f64> {
    NaiveDate::parse_from_str(as_of_date, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "instrument.as-of-date-invalid",
            format!("as-of 交易日格式无效: {as_of_date}"),
            &[as_of_date],
        )
    })?;

    let sql = "SELECT COALESCE(SUM(qty), 0.0) FROM (\
                   SELECT CASE st.action WHEN 'buy' THEN st.quantity ELSE -st.quantity END AS qty \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action IN ('buy','sell') \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
                     AND st.instrument_id = COALESCE(?2, st.instrument_id) \
                     AND (?3 IS NULL OR t.account_id = ?3) \
                     AND t.date <= ?1 \
                   UNION ALL \
                   SELECT -st.quantity AS qty \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'convert' \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
                     AND st.instrument_id = COALESCE(?2, st.instrument_id) \
                     AND (?3 IS NULL OR t.account_id = ?3) \
                     AND t.date <= ?1 \
                   UNION ALL \
                   SELECT st.to_quantity AS qty \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'convert' \
                     AND st.to_quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
                     AND st.to_instrument_id = COALESCE(?2, st.to_instrument_id) \
                     AND (?3 IS NULL OR t.account_id = ?3) \
                     AND t.date <= ?1 \
                   UNION ALL \
                   SELECT st.quantity AS qty \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'split' \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
                     AND st.instrument_id = COALESCE(?2, st.instrument_id) \
                     AND (?3 IS NULL OR t.account_id = ?3) \
                     AND t.date <= ?1) \
               ";
    let quantity: f64 =
        conn.query_row(sql, params![as_of_date, instrument_id, account_id], |r| {
            r.get(0)
        })?;
    Ok(quantity)
}

pub(crate) type HoldingsLegStream = HashMap<String, Vec<(String, f64)>>;

pub(crate) fn holdings_legs_by_instrument(conn: &Connection) -> Result<HoldingsLegStream> {
    let sql = "SELECT st.instrument_id, t.date, \
                   CASE st.action WHEN 'buy' THEN st.quantity ELSE -st.quantity END \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action IN ('buy','sell') \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
               UNION ALL \
               SELECT st.instrument_id, t.date, -st.quantity \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'convert' \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
               UNION ALL \
               SELECT st.to_instrument_id, t.date, st.to_quantity \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'convert' \
                     AND st.to_quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
               UNION ALL \
               SELECT st.instrument_id, t.date, st.quantity \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'split' \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
               ORDER BY 1, 2";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
        ))
    })?;
    let mut by_instrument: HoldingsLegStream = HashMap::new();
    for row in rows {
        let (instrument_id, date, qty) = row?;
        by_instrument
            .entry(instrument_id)
            .or_default()
            .push((date, qty));
    }
    Ok(by_instrument)
}
