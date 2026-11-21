use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use rusqlite::Connection;

use ledger_transaction::amount;

use super::fx_week::{FxWeekHistory, load_fx_week_history, nearest_week_fx_rate};
use super::model::{
    AccountPnl, CumulativePnlNativeTotal, CurrencyCumulativePnl, CurrencyHoldingTotals, PnlFilter,
    RealizedPnlSummary, YearPnl,
};
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};

pub(crate) struct LegEntry {
    pub amount_cents: i64,
    pub currency_code: String,
}

impl FromRow for LegEntry {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(LegEntry {
            amount_cents: row.get(0)?,
            currency_code: row.get(1)?,
        })
    }
}

const CUMULATIVE_PNL_LEGS: &str = "SELECT a.currency_code AS currency_code, v.unrealized_pnl_cents AS amount_cents \
     FROM v_holdings v \
     JOIN accounts a ON a.id = v.account_id \
     WHERE v.unrealized_pnl_cents IS NOT NULL \
     UNION ALL \
     SELECT sls.currency_code AS currency_code, sls.realized_pnl_cents AS amount_cents \
     FROM security_lot_sales sls \
     JOIN transactions t ON t.id = sls.sell_transaction_id \
     JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0 \
     WHERE t.is_deleted = 0 \
     UNION ALL \
     SELECT t.currency_code AS currency_code, t.amount_cents AS amount_cents \
     FROM transactions t \
     JOIN security_transactions st ON st.transaction_id = t.id AND st.action='dividend' \
     JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0 \
     WHERE t.is_deleted = 0 AND t.kind='dividend'";

pub fn query_cumulative_pnl_summary(conn: &Connection) -> Result<Vec<CurrencyCumulativePnl>> {
    let sql = format!(
        "SELECT currency_code, SUM(amount_cents) FROM ({CUMULATIVE_PNL_LEGS}) \
         GROUP BY currency_code ORDER BY currency_code"
    );
    query_all(conn, &sql, [])
}

pub fn query_cumulative_pnl_native_total(conn: &Connection) -> Result<CumulativePnlNativeTotal> {
    ensure_transaction(conn, || {
        let sql = format!("SELECT amount_cents, currency_code FROM ({CUMULATIVE_PNL_LEGS})");
        let entries: Vec<LegEntry> = query_all(conn, &sql, [])?;
        let mut total = 0i64;
        for entry in entries {
            total +=
                amount::convert_to_native_current(conn, entry.amount_cents, &entry.currency_code)?;
        }
        Ok(CumulativePnlNativeTotal {
            total_cents: total,
            native_currency: amount::default_currency_code(conn)?,
        })
    })
}

pub fn query_holdings_summary_by_currency(conn: &Connection) -> Result<Vec<CurrencyHoldingTotals>> {
    let sql = "SELECT currency_code, SUM(market_value_cents), SUM(unrealized_pnl_cents) FROM (\
                   SELECT a.currency_code AS currency_code, \
                          v.market_value_cents, v.unrealized_pnl_cents \
                   FROM v_holdings v \
                   JOIN accounts a ON a.id = v.account_id \
               ) \
               GROUP BY currency_code \
               HAVING SUM(market_value_cents) IS NOT NULL \
                   OR SUM(unrealized_pnl_cents) IS NOT NULL \
               ORDER BY currency_code";
    query_all(conn, sql, [])
}

type LegSums = BTreeMap<(String, String), i64>;

type LegGroups = BTreeMap<Vec<String>, LegSums>;

fn fetch_leg_groups(
    conn: &Connection,
    sql: &str,
    key_cols: usize,
    params: &[&dyn rusqlite::ToSql],
) -> Result<LegGroups> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| {
        let mut key = Vec::with_capacity(key_cols);
        for col in 0..key_cols {
            key.push(row.get::<_, String>(col)?);
        }
        let currency: String = row.get(key_cols)?;
        let week: Option<String> = row.get(key_cols + 1)?;
        let total: i64 = row.get::<_, Option<i64>>(key_cols + 2)?.unwrap_or(0);
        Ok((key, currency, week.unwrap_or_default(), total))
    })?;
    let mut groups = LegGroups::new();
    for row in rows {
        let (key, currency, week, total) = row?;
        *groups
            .entry(key)
            .or_default()
            .entry((currency, week))
            .or_insert(0) += total;
    }
    Ok(groups)
}

fn fold_leg_native(fx: &FxWeekHistory, native: &str, groups: &LegSums) -> Result<Option<i64>> {
    let mut total = 0i64;
    for ((currency, week), cents) in groups {
        let rate = NaiveDate::parse_from_str(week, "%Y-%m-%d")
            .ok()
            .and_then(|date| nearest_week_fx_rate(fx, currency, native, date));
        match rate {
            Some(rate) => total += (*cents as f64 * rate).round() as i64,
            None => {
                if pair_has_no_history(fx, currency, native) {
                    return Err(missing_history_pair_error(currency, native));
                }
                return Ok(None);
            }
        }
    }
    Ok(Some(total))
}

fn assemble_row(
    fx: &FxWeekHistory,
    native: &str,
    realized: &LegSums,
    dividend: &LegSums,
) -> Result<(Option<i64>, Option<i64>, Option<i64>)> {
    let realized = fold_leg_native(fx, native, realized)?;
    let dividend = fold_leg_native(fx, native, dividend)?;
    let gain = match (realized, dividend) {
        (Some(realized), Some(dividend)) => Some(realized + dividend),
        _ => None,
    };
    if gain.is_none() {
        return Ok((None, None, None));
    }
    Ok((gain, realized, dividend))
}

fn pair_has_no_history(fx: &FxWeekHistory, base: &str, quote: &str) -> bool {
    !fx.contains_key(&(base.to_string(), quote.to_string()))
        && !fx.contains_key(&(quote.to_string(), base.to_string()))
}

fn missing_history_pair_error(base: &str, quote: &str) -> AppError {
    AppError::codedp(
        "fx.rate-missing",
        format!(
            "未找到 {base} -> {quote} 的汇率（正反向均无），可在设置页「币种」页签同步汇率后重试"
        ),
        &[base, quote],
    )
}

pub fn query_realized_pnl_summary(
    conn: &Connection,
    filter: &PnlFilter,
) -> Result<RealizedPnlSummary> {
    ensure_transaction(conn, || {
        let native = amount::default_currency_code(conn)?;
        let fx = load_fx_week_history(conn)?;

        let realized_from = "FROM security_lot_sales sls \
                     JOIN transactions t ON t.id = sls.sell_transaction_id \
                     JOIN security_transactions st ON st.transaction_id = sls.sell_transaction_id \
                     JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0";
        let dividend_from = "FROM transactions t \
                         JOIN security_transactions st ON st.transaction_id = t.id AND st.action='dividend' \
                         JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0";

        let mut conditions: Vec<String> = vec!["t.is_deleted=0".to_string()];
        let mut dividend_conditions: Vec<String> = vec![
            "t.is_deleted=0".to_string(),
            "t.kind='dividend'".to_string(),
        ];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(acct_id) = &filter.account_id {
            params.push(Box::new(acct_id.clone()));
            let condition = format!("t.account_id=?{}", params.len());
            conditions.push(condition.clone());
            dividend_conditions.push(condition);
        }
        if let Some(inst_id) = &filter.instrument_id {
            params.push(Box::new(inst_id.clone()));
            let condition = format!("st.instrument_id=?{}", params.len());
            conditions.push(condition.clone());
            dividend_conditions.push(condition);
        }

        let where_clause = format!(" WHERE {}", conditions.join(" AND "));
        let dividend_where_clause = format!(" WHERE {}", dividend_conditions.join(" AND "));

        let week_of = "date(t.date, '-6 days', 'weekday 1')";
        let realized_year_sql = format!(
            "SELECT substr(t.date, 1, 4), sls.currency_code, {week_of}, SUM(sls.realized_pnl_cents) \
             {realized_from}{where_clause} \
             GROUP BY substr(t.date, 1, 4), sls.currency_code, {week_of}"
        );
        let dividend_year_sql = format!(
            "SELECT substr(t.date, 1, 4), t.currency_code, {week_of}, SUM(t.amount_cents) \
             {dividend_from}{dividend_where_clause} \
             GROUP BY substr(t.date, 1, 4), t.currency_code, {week_of}"
        );
        let realized_account_sql = format!(
            "SELECT a.id, a.name, sls.currency_code, {week_of}, SUM(sls.realized_pnl_cents) \
             {realized_from}{where_clause} \
             GROUP BY a.id, a.name, sls.currency_code, {week_of}"
        );
        let dividend_account_sql = format!(
            "SELECT a.id, a.name, t.currency_code, {week_of}, SUM(t.amount_cents) \
             {dividend_from}{dividend_where_clause} \
             GROUP BY a.id, a.name, t.currency_code, {week_of}"
        );

        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

        let realized_year = fetch_leg_groups(conn, &realized_year_sql, 1, &params_ref)?;
        let dividend_year = fetch_leg_groups(conn, &dividend_year_sql, 1, &params_ref)?;
        let realized_account = fetch_leg_groups(conn, &realized_account_sql, 2, &params_ref)?;
        let dividend_account = fetch_leg_groups(conn, &dividend_account_sql, 2, &params_ref)?;

        let empty = BTreeMap::new();
        let mut year_keys: BTreeSet<&Vec<String>> = realized_year.keys().collect();
        year_keys.extend(dividend_year.keys());
        let mut by_year = Vec::with_capacity(year_keys.len());
        for key in &year_keys {
            let (gain, realized, dividend) = assemble_row(
                &fx,
                &native,
                realized_year.get(*key).unwrap_or(&empty),
                dividend_year.get(*key).unwrap_or(&empty),
            )?;
            by_year.push(YearPnl {
                year: key[0].clone(),
                native_currency: native.clone(),
                realized_pnl_cents: realized,
                dividend_cents: dividend,
                realized_gain_cents: gain,
            });
        }

        let mut account_keys: BTreeSet<&Vec<String>> = realized_account.keys().collect();
        account_keys.extend(dividend_account.keys());
        let mut by_account = Vec::with_capacity(account_keys.len());
        for key in &account_keys {
            let (gain, realized, dividend) = assemble_row(
                &fx,
                &native,
                realized_account.get(*key).unwrap_or(&empty),
                dividend_account.get(*key).unwrap_or(&empty),
            )?;
            by_account.push(AccountPnl {
                account_id: key[0].clone(),
                account_name: key[1].clone(),
                native_currency: native.clone(),
                realized_pnl_cents: realized,
                dividend_cents: dividend,
                realized_gain_cents: gain,
            });
        }
        by_account.sort_by(|x, y| {
            x.account_name
                .cmp(&y.account_name)
                .then_with(|| x.account_id.cmp(&y.account_id))
        });

        Ok(RealizedPnlSummary {
            by_year,
            by_account,
        })
    })
}
