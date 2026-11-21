use rusqlite::Connection;

use ledger_accounts::AccountType;
use ledger_accounts::balance::list_account_balances_with_visibility;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount;

use super::model::FinancialFreedomOverview;

const SAFE_WITHDRAWAL_RATE: f64 = 0.03;

struct HoldingValue {
    amount_cents: Option<i64>,
    currency_code: String,
}

impl FromRow for HoldingValue {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(HoldingValue {
            amount_cents: row.get(0)?,
            currency_code: row.get(1)?,
        })
    }
}

pub(crate) const HOLDINGS_VISIBLE_FACET: &str = "FROM v_holdings h \
     JOIN accounts a ON a.id = h.account_id \
     WHERE a.is_deleted=0 AND a.is_hidden=0";

pub fn query_investable_assets_cash_leg_cents(conn: &Connection) -> Result<i64> {
    let mut cash_sum = 0i64;
    for ab in list_account_balances_with_visibility(conn, false)? {
        if ab.account.kind == AccountType::Investment {
            cash_sum += amount::convert_to_native_current(
                conn,
                ab.balance_cents,
                &ab.account.currency_code,
            )?;
        }
    }
    Ok(cash_sum)
}

pub(crate) fn sum_holdings_facet_column_cents(conn: &Connection, column: &str) -> Result<i64> {
    let sql = format!("SELECT h.{column}, a.currency_code {HOLDINGS_VISIBLE_FACET}");
    let holdings: Vec<HoldingValue> = query_all(conn, &sql, [])?;
    let mut sum = 0i64;
    for h in holdings {
        if let Some(value_cents) = h.amount_cents {
            sum += amount::convert_to_native_current(conn, value_cents, &h.currency_code)?;
        }
    }
    Ok(sum)
}

pub fn query_investable_assets_holdings_leg_cents(conn: &Connection) -> Result<i64> {
    sum_holdings_facet_column_cents(conn, "market_value_cents")
}

pub fn query_investable_assets_cents(conn: &Connection) -> Result<i64> {
    Ok(query_investable_assets_cash_leg_cents(conn)?
        + query_investable_assets_holdings_leg_cents(conn)?)
}

pub fn query_financial_freedom(conn: &Connection) -> Result<FinancialFreedomOverview> {
    ensure_transaction(conn, || {
        let numerator_cents = query_investable_assets_cents(conn)?;

        let denominator_cents = ledger_budget::annual_budget_total_cents(conn)?;

        let (ratio, coverage_years) = if denominator_cents == 0 {
            (0.0, 0.0)
        } else {
            (
                round1(
                    numerator_cents as f64 * SAFE_WITHDRAWAL_RATE / denominator_cents as f64
                        * 100.0,
                ),
                round1(numerator_cents as f64 / denominator_cents as f64),
            )
        };

        Ok(FinancialFreedomOverview {
            ratio,
            numerator_cents,
            denominator_cents,
            coverage_years,
            native_currency: amount::default_currency_code(conn)?,
        })
    })
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}
