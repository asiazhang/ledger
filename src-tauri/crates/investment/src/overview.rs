use rusqlite::Connection;

use ledger_accounts::AccountType;
use ledger_accounts::balance::list_accounts_with_visibility;
use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount;

use super::reports::LegEntry;

use super::financial_freedom::{
    HOLDINGS_VISIBLE_FACET, query_investable_assets_cash_leg_cents,
    query_investable_assets_holdings_leg_cents, sum_holdings_facet_column_cents,
};
use super::model::InvestmentOverview;

pub fn query_investment_overview(conn: &Connection) -> Result<InvestmentOverview> {
    ensure_transaction(conn, || {
        let investment_cash_cents = query_investable_assets_cash_leg_cents(conn)?;
        let holdings_market_value_cents = query_investable_assets_holdings_leg_cents(conn)?;
        let unrealized_pnl_cents = sum_holdings_facet_column_cents(conn, "unrealized_pnl_cents")?;
        let realized_pnl_cents = query_realized_pnl_cents(conn)?;
        let dividend_total_cents = query_dividend_total_cents(conn)?;

        Ok(InvestmentOverview {
            native_currency: amount::default_currency_code(conn)?,
            investable_assets_cents: investment_cash_cents + holdings_market_value_cents,
            investment_cash_cents,
            total_market_value_cents: holdings_market_value_cents,
            holdings_market_value_cents,
            unrealized_pnl_cents,
            cumulative_pnl_cents: unrealized_pnl_cents + realized_pnl_cents + dividend_total_cents,
            missing_price_holding_count: count_unpriced_holdings(conn)?,
            has_investment_account: has_investment_account(conn)?,
        })
    })
}

fn count_unpriced_holdings(conn: &Connection) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) {HOLDINGS_VISIBLE_FACET} AND h.market_value_cents IS NULL");
    let count = conn.query_row(&sql, [], |row| row.get(0))?;
    Ok(count)
}

fn sum_leg_entries(conn: &Connection, sql: &str) -> Result<i64> {
    let entries: Vec<LegEntry> = query_all(conn, sql, [])?;
    let mut sum = 0i64;
    for entry in entries {
        sum += amount::convert_to_native_current(conn, entry.amount_cents, &entry.currency_code)?;
    }
    Ok(sum)
}

fn query_realized_pnl_cents(conn: &Connection) -> Result<i64> {
    sum_leg_entries(
        conn,
        "SELECT sls.realized_pnl_cents, sls.currency_code \
         FROM security_lot_sales sls \
         JOIN transactions t ON t.id = sls.sell_transaction_id \
         JOIN accounts a ON a.id = t.account_id AND a.is_deleted=0 AND a.is_hidden=0 \
         WHERE t.is_deleted=0",
    )
}

fn query_dividend_total_cents(conn: &Connection) -> Result<i64> {
    sum_leg_entries(
        conn,
        "SELECT t.amount_cents, t.currency_code \
         FROM transactions t \
         JOIN security_transactions st ON st.transaction_id = t.id AND st.action='dividend' \
         JOIN accounts a ON a.id = t.account_id AND a.is_deleted=0 AND a.is_hidden=0 \
         WHERE t.is_deleted=0 AND t.kind='dividend'",
    )
}

fn has_investment_account(conn: &Connection) -> Result<bool> {
    Ok(list_accounts_with_visibility(conn, true)?
        .iter()
        .any(|account| account.kind == AccountType::Investment))
}
