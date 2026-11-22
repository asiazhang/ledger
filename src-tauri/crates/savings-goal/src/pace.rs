use chrono::{Datelike, Months, NaiveDate};
use rusqlite::Connection;
use serde::Serialize;

use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::error::Result;
use ledger_transaction::amount;

use super::model::SavingsGoalPaceSource;

struct LinkedPlanPace {
    amount_cents: i64,
    currency_code: String,
    recurrence_type: String,
    recurrence_interval: i64,
}

impl FromRow for LinkedPlanPace {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(LinkedPlanPace {
            amount_cents: row.get(0)?,
            currency_code: row.get(1)?,
            recurrence_type: row.get(2)?,
            recurrence_interval: row.get(3)?,
        })
    }
}

pub fn linked_plan_pace_monthly(conn: &Connection, account_id: &str) -> Result<Option<i64>> {
    let plans: Vec<LinkedPlanPace> = query_all(
        conn,
        "SELECT st.amount_cents,st.currency_code,st.recurrence_type,st.recurrence_interval \
         FROM scheduled_transactions st \
         JOIN scheduled_transfer_plans p ON p.scheduled_transaction_id = st.id \
         WHERE st.kind='scheduled_transfer' AND st.status='active' AND st.is_deleted=0 \
           AND p.to_account_id=?1",
        [account_id],
    )?;
    if plans.is_empty() {
        return Ok(None);
    }
    let mut monthly = 0i64;
    for p in plans {
        let recurrence_type = p.recurrence_type.parse()?;
        let native_cents =
            amount::convert_to_native_current(conn, p.amount_cents, &p.currency_code)?;
        let coefficient =
            ledger_scheduled::monthly_coefficient(recurrence_type, p.recurrence_interval);
        monthly += (native_cents as f64 * coefficient).round() as i64;
    }
    Ok(Some(monthly))
}

#[derive(Debug, Clone, Serialize)]
pub struct SavingsGoalProjection {
    pub pace_monthly_cents: Option<i64>,
    pub pace_source: Option<SavingsGoalPaceSource>,
    pub eta_months: Option<i64>,
    pub eta_month: Option<String>,
    pub required_monthly_cents: Option<i64>,
    pub pace_delta_cents: Option<i64>,
}

fn div_ceil(a: i64, b: i64) -> i64 {
    (a + b - 1) / b
}

fn month_diff(from: NaiveDate, to: NaiveDate) -> i64 {
    let month_index = |d: NaiveDate| i64::from(d.year()) * 12 + i64::from(d.month());
    month_index(to) - month_index(from)
}

pub(crate) fn project(
    deadline: Option<&str>,
    remaining_cents: i64,
    pace: Option<i64>,
    pace_source: Option<SavingsGoalPaceSource>,
    today: NaiveDate,
) -> SavingsGoalProjection {
    let mut projection = SavingsGoalProjection {
        pace_monthly_cents: pace,
        pace_source,
        eta_months: None,
        eta_month: None,
        required_monthly_cents: None,
        pace_delta_cents: None,
    };
    if remaining_cents <= 0 {
        return projection;
    }
    if deadline.is_none()
        && let Some(p) = pace
        && p > 0
    {
        let months = div_ceil(remaining_cents, p);
        projection.eta_months = Some(months);
        projection.eta_month = u32::try_from(months)
            .ok()
            .and_then(|m| today.checked_add_months(Months::new(m)))
            .map(|d| format!("{:04}-{:02}", d.year(), d.month()));
        return projection;
    }
    if let Some(deadline) = deadline
        && let Ok(d) = chrono::NaiveDate::parse_from_str(deadline, "%Y-%m-%d")
        && d >= today
    {
        let months_left = month_diff(today, d).max(1);
        let required = div_ceil(remaining_cents, months_left);
        projection.required_monthly_cents = Some(required);
        if let Some(p) = pace {
            projection.pace_delta_cents = Some(p - required);
        }
    }
    projection
}
