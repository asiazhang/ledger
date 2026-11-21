use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use super::model::BudgetAnnualSummary;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount::{Measure, contributing_kinds_sql, expense_net_expr};

pub fn annual_budget_total_cents(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN period='monthly' THEN amount_cents * 12 \
                              WHEN period='yearly' THEN amount_cents END), 0) \
         FROM budgets WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )?)
}

pub fn budget_annual_summary(conn: &Connection, today: NaiveDate) -> Result<BudgetAnnualSummary> {
    ensure_transaction(conn, || {
        let annual_budget_total_cents = annual_budget_total_cents(conn)?;

        let kinds = contributing_kinds_sql(Measure::ExpenseNet);
        let year = format!("{:04}", today.year());
        let sql = format!(
            "SELECT COALESCE(SUM({expense_net}),0) \
             FROM transactions t \
             JOIN categories tc ON tc.id=t.category_id \
             WHERE t.kind IN ({kinds}) AND t.is_deleted=0 AND substr(t.date,1,4)=?1 \
               AND (EXISTS (SELECT 1 FROM budgets b WHERE b.category_id=tc.id AND b.is_deleted=0) \
                 OR EXISTS (SELECT 1 FROM budgets bp WHERE bp.category_id=tc.parent_id AND bp.is_deleted=0))",
            expense_net = expense_net_expr("t"),
        );
        let spent_cents: i64 = conn.query_row(&sql, rusqlite::params![year], |r| r.get(0))?;

        Ok(BudgetAnnualSummary {
            annual_budget_total_cents,
            spent_cents,
            over_budget: spent_cents > annual_budget_total_cents,
        })
    })
}
