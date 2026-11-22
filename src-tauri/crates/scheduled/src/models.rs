use serde::{Deserialize, Serialize};

use ledger_infra::db::query::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledKind {
    Installment,
    Subscription,
    #[serde(rename = "scheduled_transfer")]
    ScheduledTransfer,
}

impl std::fmt::Display for ScheduledKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduledKind::Installment => write!(f, "installment"),
            ScheduledKind::Subscription => write!(f, "subscription"),
            ScheduledKind::ScheduledTransfer => write!(f, "scheduled_transfer"),
        }
    }
}

impl std::str::FromStr for ScheduledKind {
    type Err = ledger_infra::error::AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "installment" => Ok(ScheduledKind::Installment),
            "subscription" => Ok(ScheduledKind::Subscription),
            "scheduled_transfer" => Ok(ScheduledKind::ScheduledTransfer),
            _ => Err(ledger_infra::error::AppError::codedp(
                "scheduled-plan.kind-unknown",
                format!("未知定时交易类型: {s}"),
                &[s],
            )),
        }
    }
}

impl rusqlite::types::FromSql for ScheduledKind {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = value.as_str()?;
        s.parse().map_err(|e: ledger_infra::error::AppError| {
            rusqlite::types::FromSqlError::Other(Box::new(e))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledStatus {
    Active,
    Paused,
    Cancelled,
    Completed,
}

impl std::fmt::Display for ScheduledStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduledStatus::Active => write!(f, "active"),
            ScheduledStatus::Paused => write!(f, "paused"),
            ScheduledStatus::Cancelled => write!(f, "cancelled"),
            ScheduledStatus::Completed => write!(f, "completed"),
        }
    }
}

impl std::str::FromStr for ScheduledStatus {
    type Err = ledger_infra::error::AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(ScheduledStatus::Active),
            "paused" => Ok(ScheduledStatus::Paused),
            "cancelled" => Ok(ScheduledStatus::Cancelled),
            "completed" => Ok(ScheduledStatus::Completed),
            _ => Err(ledger_infra::error::AppError::codedp(
                "scheduled-plan.status-unknown",
                format!("未知计划状态: {s}"),
                &[s],
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecurrenceType {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl std::fmt::Display for RecurrenceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecurrenceType::Daily => write!(f, "daily"),
            RecurrenceType::Weekly => write!(f, "weekly"),
            RecurrenceType::Monthly => write!(f, "monthly"),
            RecurrenceType::Yearly => write!(f, "yearly"),
        }
    }
}

impl std::str::FromStr for RecurrenceType {
    type Err = ledger_infra::error::AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "daily" => Ok(RecurrenceType::Daily),
            "weekly" => Ok(RecurrenceType::Weekly),
            "monthly" => Ok(RecurrenceType::Monthly),
            "yearly" => Ok(RecurrenceType::Yearly),
            _ => Err(ledger_infra::error::AppError::codedp(
                "scheduled-plan.recurrence-unknown",
                format!("未知周期类型: {s}"),
                &[s],
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum OccurrenceStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for OccurrenceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OccurrenceStatus::Pending => write!(f, "pending"),
            OccurrenceStatus::Processing => write!(f, "processing"),
            OccurrenceStatus::Completed => write!(f, "completed"),
            OccurrenceStatus::Failed => write!(f, "failed"),
            OccurrenceStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::str::FromStr for OccurrenceStatus {
    type Err = ledger_infra::error::AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(OccurrenceStatus::Pending),
            "processing" => Ok(OccurrenceStatus::Processing),
            "completed" => Ok(OccurrenceStatus::Completed),
            "failed" => Ok(OccurrenceStatus::Failed),
            "cancelled" => Ok(OccurrenceStatus::Cancelled),
            _ => Err(ledger_infra::error::AppError::codedp(
                "scheduled-occurrence.status-unknown",
                format!("未知期次状态: {s}"),
                &[s],
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTransaction {
    pub id: String,
    pub kind: ScheduledKind,
    pub status: String,
    pub account_id: String,
    pub category_id: Option<String>,
    pub amount_cents: i64,
    pub currency_code: String,
    pub recurrence_type: String,
    pub recurrence_interval: i64,
    pub recurrence_day: Option<i64>,
    pub start_date: String,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTransactionOccurrence {
    pub id: String,
    pub scheduled_transaction_id: String,
    pub scheduled_date: String,
    pub status: String,
    pub transaction_id: Option<String>,
    pub amount_cents: i64,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallmentPlan {
    pub scheduled_transaction_id: String,
    pub merchant_id: Option<String>,
    pub total_amount_cents: i64,
    pub total_occurrences: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPlan {
    pub scheduled_transaction_id: String,
    pub merchant_id: Option<String>,
    pub policy_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTransferPlan {
    pub scheduled_transaction_id: String,
    pub to_account_id: String,
    pub total_occurrences: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CreateScheduledInput {
    pub kind: ScheduledKind,
    pub account_id: String,
    pub category_id: Option<String>,
    pub amount_cents: i64,
    pub currency_code: String,
    pub recurrence_type: RecurrenceType,
    pub recurrence_interval: i64,
    pub recurrence_day: Option<i64>,
    pub start_date: String,
    pub note: Option<String>,
    pub merchant_id: Option<String>,
    pub policy_id: Option<String>,
    pub total_amount_cents: Option<i64>,
    pub total_occurrences: Option<i64>,
    pub to_account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStatusInput {
    pub id: String,
    pub new_status: ScheduledStatus,
}

fn de_amount_sentinel<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer
        .deserialize_any(serde::de::IgnoredAny)
        .map(|_| true)
}

#[derive(Debug, Deserialize)]
pub struct UpdateSubscriptionInput {
    pub id: String,
    pub account_id: String,
    pub category_id: Option<String>,
    pub note: Option<String>,
    pub merchant_id: Option<String>,
    #[serde(default, deserialize_with = "de_amount_sentinel")]
    pub amount_cents: bool,
    #[serde(default, deserialize_with = "de_amount_sentinel")]
    pub total_amount_cents: bool,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteOccurrenceInput {
    pub occurrence_id: String,
}

#[derive(Debug, Serialize)]
pub struct ScheduledTransactionWithExt {
    pub core: ScheduledTransaction,
    pub merchant_id: Option<String>,
    pub policy_id: Option<String>,
    pub total_amount_cents: Option<i64>,
    pub total_occurrences: Option<i64>,
    pub to_account_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ScheduledTransactionDetail {
    pub core: ScheduledTransaction,
    pub extension: serde_json::Value,
    pub pending_occurrences: Vec<ScheduledTransactionOccurrence>,
    pub completed_occurrences: i64,
    pub completed_amount_cents: i64,
    pub occurrences: Vec<ScheduledTransactionOccurrence>,
}

impl FromRow for ScheduledTransaction {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ScheduledTransaction {
            id: row.get(0)?,
            kind: row.get(1)?,
            status: row.get(2)?,
            account_id: row.get(3)?,
            category_id: row.get(4)?,
            amount_cents: row.get(5)?,
            currency_code: row.get(6)?,
            recurrence_type: row.get(7)?,
            recurrence_interval: row.get(8)?,
            recurrence_day: row.get(9)?,
            start_date: row.get(10)?,
            note: row.get(11)?,
            created_at: row.get(12)?,
            updated_at: row.get(13)?,
            version: row.get(14)?,
            device_id: row.get(15)?,
            is_deleted: row.get::<_, i64>(16)? != 0,
        })
    }
}

impl FromRow for ScheduledTransactionOccurrence {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ScheduledTransactionOccurrence {
            id: row.get(0)?,
            scheduled_transaction_id: row.get(1)?,
            scheduled_date: row.get(2)?,
            status: row.get(3)?,
            transaction_id: row.get(4)?,
            amount_cents: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            version: row.get(8)?,
            device_id: row.get(9)?,
            is_deleted: row.get::<_, i64>(10)? != 0,
        })
    }
}

impl FromRow for InstallmentPlan {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(InstallmentPlan {
            scheduled_transaction_id: row.get(0)?,
            merchant_id: row.get(1)?,
            total_amount_cents: row.get(2)?,
            total_occurrences: row.get(3)?,
        })
    }
}

impl FromRow for SubscriptionPlan {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(SubscriptionPlan {
            scheduled_transaction_id: row.get(0)?,
            merchant_id: row.get(1)?,
            policy_id: row.get(2)?,
        })
    }
}

impl FromRow for ScheduledTransferPlan {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ScheduledTransferPlan {
            scheduled_transaction_id: row.get(0)?,
            to_account_id: row.get(1)?,
            total_occurrences: row.get(2)?,
        })
    }
}
