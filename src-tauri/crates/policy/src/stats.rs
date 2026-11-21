use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::Connection;

use super::model::PolicyStats;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount::{
    Measure, contributing_kinds_sql, policy_inflow_expr, policy_premium_expr,
};

use super::validation::parse_date;

struct PolicyPeriodRow {
    id: String,
    end_date: Option<String>,
}

impl FromRow for PolicyPeriodRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PolicyPeriodRow {
            id: row.get(0)?,
            end_date: row.get(1)?,
        })
    }
}

struct PolicySumRow {
    policy_id: String,
    native_cents: i64,
}

impl FromRow for PolicySumRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PolicySumRow {
            policy_id: row.get(0)?,
            native_cents: row.get(1)?,
        })
    }
}

struct PolicyNextChargeRow {
    policy_id: String,
    next_date: String,
}

impl FromRow for PolicyNextChargeRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PolicyNextChargeRow {
            policy_id: row.get(0)?,
            next_date: row.get(1)?,
        })
    }
}

fn sum_by_policy(
    conn: &Connection,
    measure_expr: &str,
    kinds_sql: &str,
) -> Result<HashMap<String, i64>> {
    Ok(query_all::<PolicySumRow, _>(
        conn,
        &format!(
            "SELECT t.policy_id, SUM({measure_expr}) AS native_cents \
                 FROM transactions t JOIN policies p ON p.id = t.policy_id \
                 WHERE t.is_deleted=0 AND t.kind IN ({kinds_sql}) AND p.is_deleted=0 \
                 GROUP BY t.policy_id",
        ),
        [],
    )?
    .into_iter()
    .map(|r| (r.policy_id, r.native_cents))
    .collect())
}

pub fn policy_stats(conn: &Connection, today: NaiveDate) -> Result<Vec<PolicyStats>> {
    ensure_transaction(conn, || policy_stats_within_tx(conn, today))
}

fn policy_stats_within_tx(conn: &Connection, today: NaiveDate) -> Result<Vec<PolicyStats>> {
    let periods: Vec<PolicyPeriodRow> = query_all(
        conn,
        "SELECT id, end_date FROM policies WHERE is_deleted=0 ORDER BY created_at, id",
        [],
    )?;

    let paid = sum_by_policy(
        conn,
        &policy_premium_expr("t"),
        &contributing_kinds_sql(Measure::PolicyPremium),
    )?;
    let inflow = sum_by_policy(
        conn,
        &policy_inflow_expr("t"),
        &contributing_kinds_sql(Measure::PolicyInflow),
    )?;

    let next_charges: HashMap<String, String> = query_all::<PolicyNextChargeRow, _>(
        conn,
        "SELECT sp.policy_id, MIN(o.scheduled_date) AS next_date \
         FROM scheduled_transaction_occurrences o \
         JOIN scheduled_transactions st ON st.id = o.scheduled_transaction_id \
         JOIN subscription_plans sp ON sp.scheduled_transaction_id = st.id \
         WHERE sp.policy_id IS NOT NULL AND st.is_deleted=0 \
           AND st.kind='subscription' AND st.status='active' \
           AND o.is_deleted=0 AND o.status='pending' \
         GROUP BY sp.policy_id",
        [],
    )?
    .into_iter()
    .map(|r| (r.policy_id, r.next_date))
    .collect();

    periods
        .into_iter()
        .map(|p| {
            let is_expired = match &p.end_date {
                Some(end) => {
                    let end_date = parse_date(end)?;
                    end_date < today
                }
                None => false,
            };
            let policy_id = p.id;
            let total_paid_native_cents = paid.get(&policy_id).copied().unwrap_or(0);
            let total_inflow_native_cents = inflow.get(&policy_id).copied().unwrap_or(0);
            let next_charge_date = next_charges.get(&policy_id).cloned();
            Ok(PolicyStats {
                policy_id,
                native_currency: ledger_transaction::amount::default_currency_code(conn)?
                    .to_string(),
                total_paid_native_cents,
                total_inflow_native_cents,
                next_charge_date,
                is_expired,
            })
        })
        .collect()
}
