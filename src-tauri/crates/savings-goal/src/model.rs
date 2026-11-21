use std::fmt;
use std::str::FromStr;

use ledger_infra::error::AppError;
use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavingsGoalStatus {
    Active,
    Archived,
}

impl fmt::Display for SavingsGoalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SavingsGoalStatus::Active => write!(f, "active"),
            SavingsGoalStatus::Archived => write!(f, "archived"),
        }
    }
}

impl FromStr for SavingsGoalStatus {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(SavingsGoalStatus::Active),
            "archived" => Ok(SavingsGoalStatus::Archived),
            _ => Err(AppError::codedp(
                "savings-goal.status-unknown",
                format!("未知目标状态: {s}"),
                &[s],
            )),
        }
    }
}

impl ToSql for SavingsGoalStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for SavingsGoalStatus {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        value
            .as_str()?
            .parse()
            .map_err(|e: AppError| FromSqlError::Other(Box::new(e)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavingsGoal {
    pub id: String,
    pub name: String,
    pub target_amount_cents: i64,
    pub deadline: Option<String>,
    pub status: SavingsGoalStatus,
    pub planned_monthly_cents: Option<i64>,
    pub account_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, Deserialize)]
pub struct SavingsGoalInput {
    pub name: String,
    pub target_amount_cents: i64,
    pub deadline: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SavingsGoalUpdateInput {
    pub name: String,
    pub target_amount_cents: i64,
    pub deadline: Option<String>,
    pub planned_monthly_cents: Option<i64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavingsGoalPaceSource {
    Plan,
    Manual,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavingsGoalProgress {
    pub goal: SavingsGoal,
    pub saved_cents: i64,
    pub remaining_cents: i64,
    pub achieved: bool,
    pub currency_code: String,
    pub pace_monthly_cents: Option<i64>,
    pub pace_source: Option<SavingsGoalPaceSource>,
    pub eta_months: Option<i64>,
    pub eta_month: Option<String>,
    pub required_monthly_cents: Option<i64>,
    pub pace_delta_cents: Option<i64>,
}

pub(crate) fn goal_from_row(row: &rusqlite::Row) -> rusqlite::Result<SavingsGoal> {
    Ok(SavingsGoal {
        id: row.get(0)?,
        name: row.get(1)?,
        target_amount_cents: row.get(2)?,
        deadline: row.get(3)?,
        status: row.get(4)?,
        planned_monthly_cents: row.get(5)?,
        account_id: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        version: row.get(9)?,
        device_id: row.get(10)?,
        is_deleted: row.get::<_, i64>(11)? != 0,
    })
}
