use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use super::model::BudgetProgress;
use ledger_infra::db::query::query_all;
use ledger_infra::error::Result;
use ledger_transaction::amount::{Measure, contributing_kinds_sql, expense_net_expr};

pub fn budget_progress_rows(conn: &Connection, today: NaiveDate) -> Result<Vec<BudgetProgress>> {
    let kinds = contributing_kinds_sql(Measure::ExpenseNet);
    let month = format!("{:04}-{:02}", today.year(), today.month());
    let year = format!("{:04}", today.year());
    let sql = format!(
        "SELECT b.id,b.category_id,b.period,b.amount_cents,b.start_date,b.created_at,b.updated_at,b.version,b.device_id,b.is_deleted,c.name, \
         COALESCE((SELECT SUM({expense_net}) \
                   FROM transactions t \
                   JOIN categories tc ON tc.id=t.category_id \
                   WHERE (tc.id=b.category_id OR tc.parent_id=b.category_id) \
                     AND t.kind IN ({kinds}) \
                     AND t.is_deleted=0 \
                     AND (CASE WHEN b.period='monthly' THEN substr(t.date,1,7)=?1 \
                               ELSE substr(t.date,1,4)=?2 END)),0) \
         FROM budgets b LEFT JOIN categories c ON c.id=b.category_id \
         WHERE b.is_deleted=0 ORDER BY b.created_at",
        expense_net = expense_net_expr("t"),
    );
    query_all(conn, &sql, rusqlite::params![month, year])
}
