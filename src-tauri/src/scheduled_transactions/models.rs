use serde::{Deserialize, Serialize};

use crate::db::query::FromRow;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

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
    type Err = crate::error::AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "installment" => Ok(ScheduledKind::Installment),
            "subscription" => Ok(ScheduledKind::Subscription),
            "scheduled_transfer" => Ok(ScheduledKind::ScheduledTransfer),
            _ => Err(crate::error::AppError::Invalid(format!(
                "未知定时交易类型: {s}"
            ))),
        }
    }
}

// rusqlite：从 `scheduled_transactions.kind` 列直接读为枚举（DB 边界）。
impl rusqlite::types::FromSql for ScheduledKind {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = value.as_str()?;
        s.parse()
            .map_err(|e: crate::error::AppError| rusqlite::types::FromSqlError::Other(Box::new(e)))
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
    type Err = crate::error::AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(ScheduledStatus::Active),
            "paused" => Ok(ScheduledStatus::Paused),
            "cancelled" => Ok(ScheduledStatus::Cancelled),
            "completed" => Ok(ScheduledStatus::Completed),
            _ => Err(crate::error::AppError::Invalid(format!(
                "未知计划状态: {s}"
            ))),
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
    type Err = crate::error::AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "daily" => Ok(RecurrenceType::Daily),
            "weekly" => Ok(RecurrenceType::Weekly),
            "monthly" => Ok(RecurrenceType::Monthly),
            "yearly" => Ok(RecurrenceType::Yearly),
            _ => Err(crate::error::AppError::Invalid(format!(
                "未知周期类型: {s}"
            ))),
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
    type Err = crate::error::AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(OccurrenceStatus::Pending),
            "processing" => Ok(OccurrenceStatus::Processing),
            "completed" => Ok(OccurrenceStatus::Completed),
            "failed" => Ok(OccurrenceStatus::Failed),
            "cancelled" => Ok(OccurrenceStatus::Cancelled),
            _ => Err(crate::error::AppError::Invalid(format!(
                "未知期次状态: {s}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Core model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTransaction {
    pub id: String,
    /// 定时交易类型枚举（serde 小写字符串序列化，wire 与裸 String 一致）。
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

// ---------------------------------------------------------------------------
// Occurrence
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Extension models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallmentPlan {
    pub scheduled_transaction_id: String,
    pub counterparty: Option<String>,
    pub total_amount_cents: i64,
    pub total_occurrences: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPlan {
    pub scheduled_transaction_id: String,
    pub counterparty: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTransferPlan {
    pub scheduled_transaction_id: String,
    pub to_account_id: String,
    pub total_occurrences: Option<i64>,
}

// ---------------------------------------------------------------------------
// Input / output types for commands
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
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
    // Type-specific
    pub counterparty: Option<String>,
    pub total_amount_cents: Option<i64>,
    pub total_occurrences: Option<i64>,
    pub to_account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStatusInput {
    pub id: String,
    pub new_status: ScheduledStatus,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteOccurrenceInput {
    pub occurrence_id: String,
}

#[derive(Debug, Serialize)]
pub struct ScheduledTransactionWithExt {
    pub core: ScheduledTransaction,
    pub counterparty: Option<String>,
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
}

// ---------------------------------------------------------------------------
// FromRow implementations
// ---------------------------------------------------------------------------

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
            counterparty: row.get(1)?,
            total_amount_cents: row.get(2)?,
            total_occurrences: row.get(3)?,
        })
    }
}

impl FromRow for SubscriptionPlan {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(SubscriptionPlan {
            scheduled_transaction_id: row.get(0)?,
            counterparty: row.get(1)?,
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
