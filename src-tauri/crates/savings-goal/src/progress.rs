use chrono::NaiveDate;
use rusqlite::Connection;

use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;

use super::model::{SavingsGoal, SavingsGoalPaceSource, SavingsGoalProgress, goal_from_row};
use super::pace::{linked_plan_pace_monthly, project};

struct GoalWithCurrency {
    goal: SavingsGoal,
    currency_code: String,
}

impl FromRow for GoalWithCurrency {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            goal: goal_from_row(row)?,
            currency_code: row.get(12)?,
        })
    }
}

pub fn list_savings_goal_progress(
    conn: &Connection,
    today: NaiveDate,
) -> Result<Vec<SavingsGoalProgress>> {
    ensure_transaction(conn, || list_savings_goal_progress_within_tx(conn, today))
}

fn list_savings_goal_progress_within_tx(
    conn: &Connection,
    today: NaiveDate,
) -> Result<Vec<SavingsGoalProgress>> {
    let goals: Vec<GoalWithCurrency> = query_all(
        conn,
        "SELECT g.id,g.name,g.target_amount_cents,g.deadline,g.status,g.planned_monthly_cents, \
         g.account_id,g.created_at,g.updated_at,g.version,g.device_id,g.is_deleted,a.currency_code \
         FROM goals g JOIN accounts a ON a.id = g.account_id \
         WHERE g.is_deleted=0 ORDER BY g.created_at",
        [],
    )?;
    let mut progress = Vec::new();
    for GoalWithCurrency {
        goal,
        currency_code,
    } in goals
    {
        let saved_cents = ledger_accounts::balance::cached_balance(conn, &goal.account_id)?;
        let remaining_cents = goal.target_amount_cents - saved_cents;
        let (pace_monthly_cents, pace_source) =
            match linked_plan_pace_monthly(conn, &goal.account_id)? {
                Some(monthly) => (Some(monthly), Some(SavingsGoalPaceSource::Plan)),
                None => (
                    goal.planned_monthly_cents,
                    goal.planned_monthly_cents
                        .map(|_| SavingsGoalPaceSource::Manual),
                ),
            };
        let projection = project(
            goal.deadline.as_deref(),
            remaining_cents,
            pace_monthly_cents,
            pace_source,
            today,
        );
        progress.push(SavingsGoalProgress {
            achieved: saved_cents >= goal.target_amount_cents,
            goal,
            saved_cents,
            remaining_cents,
            currency_code,
            pace_monthly_cents: projection.pace_monthly_cents,
            pace_source: projection.pace_source,
            eta_months: projection.eta_months,
            eta_month: projection.eta_month,
            required_monthly_cents: projection.required_monthly_cents,
            pace_delta_cents: projection.pace_delta_cents,
        });
    }
    Ok(progress)
}
