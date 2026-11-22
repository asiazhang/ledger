use std::collections::HashMap;

use chrono::{Datelike, Months, NaiveDate};
use rusqlite::Connection;
use serde::Serialize;

use super::models::RecurrenceType;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount;

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionSpendRow {
    pub plan_id: String,
    pub note: Option<String>,
    pub merchant_name: Option<String>,
    pub status: String,
    pub amount_cents: i64,
    pub currency_code: String,
    pub this_month_native_cents: i64,
    pub this_year_native_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionMonthSpend {
    pub month: String,
    pub native_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionSpendOverview {
    pub native_currency: String,
    pub this_month_native_cents: i64,
    pub this_year_native_cents: i64,
    pub months: Vec<SubscriptionMonthSpend>,
    pub rows: Vec<SubscriptionSpendRow>,
    pub projected_month_native_cents: i64,
    pub projected_year_native_cents: i64,
}

struct PlanMonthSpend {
    plan_id: String,
    month: String,
    native_cents: i64,
}

impl FromRow for PlanMonthSpend {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PlanMonthSpend {
            plan_id: row.get(0)?,
            month: row.get(1)?,
            native_cents: row.get(2)?,
        })
    }
}

struct PlanBase {
    id: String,
    note: Option<String>,
    merchant_name: Option<String>,
    status: String,
    amount_cents: i64,
    currency_code: String,
}

impl FromRow for PlanBase {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PlanBase {
            id: row.get(0)?,
            note: row.get(1)?,
            merchant_name: row.get(2)?,
            status: row.get(3)?,
            amount_cents: row.get(4)?,
            currency_code: row.get(5)?,
        })
    }
}

pub fn monthly_coefficient(recurrence_type: RecurrenceType, recurrence_interval: i64) -> f64 {
    let per_cycle = match recurrence_type {
        RecurrenceType::Monthly => 1.0,
        RecurrenceType::Yearly => 1.0 / 12.0,
        RecurrenceType::Weekly => 52.0 / 12.0,
        RecurrenceType::Daily => 30.0,
    };
    per_cycle / recurrence_interval as f64
}

struct PlanBilling {
    amount_cents: i64,
    currency_code: String,
    recurrence_type: String,
    recurrence_interval: i64,
}

impl FromRow for PlanBilling {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PlanBilling {
            amount_cents: row.get(0)?,
            currency_code: row.get(1)?,
            recurrence_type: row.get(2)?,
            recurrence_interval: row.get(3)?,
        })
    }
}

fn query_projected_cost(conn: &Connection) -> Result<(i64, i64)> {
    let plans: Vec<PlanBilling> = query_all(
        conn,
        "SELECT amount_cents, currency_code, recurrence_type, recurrence_interval \
         FROM scheduled_transactions \
         WHERE kind = 'subscription' AND status = 'active' AND is_deleted = 0",
        [],
    )?;
    let mut projected_month = 0i64;
    for p in plans {
        let recurrence_type: RecurrenceType = p.recurrence_type.parse()?;
        let native_cents =
            amount::convert_to_native_current(conn, p.amount_cents, &p.currency_code)?;
        let coefficient = monthly_coefficient(recurrence_type, p.recurrence_interval);
        projected_month += (native_cents as f64 * coefficient).round() as i64;
    }
    Ok((projected_month, projected_month * 12))
}

pub fn query_subscription_spend(
    conn: &Connection,
    today: NaiveDate,
) -> Result<SubscriptionSpendOverview> {
    ensure_transaction(conn, || query_subscription_spend_within_tx(conn, today))
}

fn query_subscription_spend_within_tx(
    conn: &Connection,
    today: NaiveDate,
) -> Result<SubscriptionSpendOverview> {
    let spends: Vec<PlanMonthSpend> = query_all(
        conn,
        &format!(
            "SELECT st.id, substr(t.date,1,7) AS month, SUM({}) AS native_cents \
             FROM scheduled_transaction_occurrences o \
             JOIN scheduled_transactions st ON st.id = o.scheduled_transaction_id \
             JOIN transactions t ON t.id = o.transaction_id \
             WHERE st.kind = 'subscription' AND o.status = 'completed' \
               AND o.is_deleted = 0 AND st.is_deleted = 0 AND t.is_deleted = 0 \
             GROUP BY st.id, month",
            amount::expense_net_expr("t")
        ),
        [],
    )?;

    let mut by_plan: HashMap<String, HashMap<String, i64>> = HashMap::new();
    for s in spends {
        by_plan
            .entry(s.plan_id)
            .or_default()
            .insert(s.month, s.native_cents);
    }

    let first = today - Months::new(11);
    let mut months = Vec::with_capacity(12);
    let mut cursor = first;
    for _ in 0..12 {
        months.push(SubscriptionMonthSpend {
            month: format!("{:04}-{:02}", cursor.year(), cursor.month()),
            native_cents: 0,
        });
        cursor = cursor + Months::new(1);
    }

    let this_month_key = format!("{:04}-{:02}", today.year(), today.month());
    let this_year_prefix = format!("{:04}", today.year());

    let mut this_month_native_cents = 0i64;
    let mut this_year_native_cents = 0i64;
    for m in &mut months {
        for plan_map in by_plan.values() {
            if let Some(cents) = plan_map.get(&m.month) {
                m.native_cents += cents;
            }
        }
        if m.month == this_month_key {
            this_month_native_cents = m.native_cents;
        }
        if m.month.starts_with(&this_year_prefix) {
            this_year_native_cents += m.native_cents;
        }
    }

    let bases: Vec<PlanBase> = query_all(
        conn,
        "SELECT st.id, st.note, m.name, st.status, st.amount_cents, st.currency_code \
         FROM scheduled_transactions st \
         LEFT JOIN subscription_plans sp ON sp.scheduled_transaction_id = st.id \
         LEFT JOIN merchants m ON m.id = sp.merchant_id \
         WHERE st.kind = 'subscription' AND st.is_deleted = 0 \
         ORDER BY st.created_at DESC",
        [],
    )?;

    let rows = bases
        .into_iter()
        .map(|b| {
            let plan_map = by_plan.get(&b.id);
            let month_cents =
                |month: &str| -> i64 { plan_map.and_then(|m| m.get(month)).copied().unwrap_or(0) };
            let window_sum = |pred: &dyn Fn(&SubscriptionMonthSpend) -> bool| -> i64 {
                months
                    .iter()
                    .filter(|m| pred(m))
                    .map(|m| month_cents(&m.month))
                    .sum()
            };
            SubscriptionSpendRow {
                plan_id: b.id,
                note: b.note,
                merchant_name: b.merchant_name,
                status: b.status,
                amount_cents: b.amount_cents,
                currency_code: b.currency_code,
                this_month_native_cents: window_sum(&|m| m.month == this_month_key),
                this_year_native_cents: window_sum(&|m| m.month.starts_with(&this_year_prefix)),
            }
        })
        .collect();

    let (projected_month_native_cents, projected_year_native_cents) = query_projected_cost(conn)?;

    Ok(SubscriptionSpendOverview {
        native_currency: amount::default_currency_code(conn)?,
        this_month_native_cents,
        this_year_native_cents,
        months,
        rows,
        projected_month_native_cents,
        projected_year_native_cents,
    })
}
