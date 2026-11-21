#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

#[cfg(test)]
mod tests;

mod model;

pub use model::{CategoryShare, DateRange, MerchantShare, MerchantSharesReport, MonthlySummary};

use rusqlite::Connection;

use ledger_infra::db::query::query_all;
use ledger_infra::error::Result;
use ledger_transaction::amount::{
    Measure, contributing_kinds_sql, expense_gross_expr, expense_net_expr, income_net_expr,
    refund_gross_expr,
};

fn push_period_conditions(
    sql: &mut String,
    params: &mut Vec<String>,
    column: &str,
    from: Option<&str>,
    to: Option<&str>,
) {
    if let Some(f) = from {
        params.push(f.to_string());
        sql.push_str(&format!(" AND {column}>=?{}", params.len()));
    }
    if let Some(t) = to {
        params.push(t.to_string());
        sql.push_str(&format!(" AND {column}<=?{}", params.len()));
    }
}

pub fn monthly_summary_rows(
    conn: &Connection,
    year: i64,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<MonthlySummary>> {
    let mut sql = format!(
        "SELECT substr(date,1,7) AS month, \
         SUM({income}) AS income, \
         SUM({expense_gross}) AS expense, \
         SUM({refund_gross}) AS refund \
         FROM transactions INDEXED BY idx_transactions_month_expr WHERE is_deleted=0",
        income = income_net_expr("transactions"),
        expense_gross = expense_gross_expr("transactions"),
        refund_gross = refund_gross_expr("transactions"),
    );
    let mut params: Vec<String> = Vec::new();
    push_period_conditions(&mut sql, &mut params, "date", from, to);
    if params.is_empty() {
        params.push(format!("{year}"));
        sql.push_str(" AND substr(date,1,4)=?1");
    }
    sql.push_str(" GROUP BY month ORDER BY month");
    query_all(conn, &sql, rusqlite::params_from_iter(params))
}

pub fn category_shares_rows(
    conn: &Connection,
    kind: &str,
    month: Option<&str>,
    year: Option<i64>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<CategoryShare>> {
    let (measure, expr) = if kind == "expense" {
        (Measure::ExpenseNet, expense_net_expr("t"))
    } else {
        (Measure::IncomeNet, income_net_expr("t"))
    };
    let kinds = contributing_kinds_sql(measure);
    let mut sql = format!(
        "SELECT t.category_id, COALESCE(c.name,'未分类'), SUM({expr}) AS net \
         FROM transactions t INDEXED BY idx_transactions_category_covering \
         LEFT JOIN categories c ON c.id=t.category_id \
         WHERE t.kind IN ({kinds}) AND t.is_deleted=0"
    );
    let mut params: Vec<String> = Vec::new();
    if from.is_some() || to.is_some() {
        push_period_conditions(&mut sql, &mut params, "t.date", from, to);
    } else {
        if let Some(m) = month {
            params.push(m.to_string());
            sql.push_str(&format!(" AND substr(t.date,1,7)=?{}", params.len()));
        }
        if let Some(y) = year {
            params.push(y.to_string());
            sql.push_str(&format!(" AND substr(t.date,1,4)=?{}", params.len()));
        }
    }
    sql.push_str(" GROUP BY t.category_id ORDER BY net DESC");
    query_all(conn, &sql, rusqlite::params_from_iter(params))
}

pub fn merchant_shares_report(
    conn: &Connection,
    year: i64,
    from: Option<&str>,
    to: Option<&str>,
    top_n: Option<i64>,
) -> Result<MerchantSharesReport> {
    let kinds = contributing_kinds_sql(Measure::ExpenseNet);
    let mut sql = format!(
        "SELECT t.merchant_id, m.name, SUM({expr}) AS net, COUNT(*) AS transaction_count \
         FROM transactions t INDEXED BY idx_transactions_merchant_covering \
         JOIN merchants m ON m.id=t.merchant_id \
         WHERE t.kind IN ({kinds}) AND t.is_deleted=0",
        expr = expense_net_expr("t"),
    );
    let mut params: Vec<String> = Vec::new();
    push_period_conditions(&mut sql, &mut params, "t.date", from, to);
    if params.is_empty() {
        params.push(format!("{year}"));
        sql.push_str(" AND substr(t.date,1,4)=?1");
    }
    sql.push_str(" GROUP BY t.merchant_id ORDER BY net DESC, m.name");
    let rows: Vec<MerchantShare> = query_all(conn, &sql, rusqlite::params_from_iter(params))?;
    let total_cents = rows.iter().map(|r| r.amount_cents).sum();
    let rows = match top_n.map(|n| usize::try_from(n).unwrap_or(0)) {
        Some(limit) => rows.into_iter().take(limit).collect(),
        None => rows,
    };
    Ok(MerchantSharesReport { rows, total_cents })
}

pub fn query_report_date_range(conn: &Connection) -> Result<DateRange> {
    let (min_date, max_date): (Option<String>, Option<String>) = conn.query_row(
        "SELECT (SELECT MIN(date) FROM transactions WHERE is_deleted=0), \
         (SELECT MAX(date) FROM transactions WHERE is_deleted=0)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(DateRange { min_date, max_date })
}
