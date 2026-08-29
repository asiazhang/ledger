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
    /// 商户引用（issue #190 / ADR-0028）：counterparty 文本列改 merchant_id，硬删置空。
    pub merchant_id: Option<String>,
    pub total_amount_cents: i64,
    pub total_occurrences: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPlan {
    pub scheduled_transaction_id: String,
    /// 商户引用（issue #190 / ADR-0028）。
    pub merchant_id: Option<String>,
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
    // Type-specific（issue #190 / ADR-0028）：installment/subscription 携带商户；
    // scheduled_transfer 行为层拒绝携带（见 engine::create_plan）。
    pub merchant_id: Option<String>,
    pub total_amount_cents: Option<i64>,
    pub total_occurrences: Option<i64>,
    pub to_account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStatusInput {
    pub id: String,
    pub new_status: ScheduledStatus,
}

/// 金额哨兵字段反序列化（ADR-0023 决策三）：请求体中该 key 一旦出现
/// （含显式 `null`）即记为 `true`，由领域函数显式拒绝。
/// 不用 `Option<i64>`：显式 `null` 会被反序列化为 `None` 而漏过拒绝。
fn de_amount_sentinel<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer
        .deserialize_any(serde::de::IgnoredAny)
        .map(|_| true)
}

/// 订阅编辑输入（issue #162，ADR-0023 决策三）：仅允许金额以外字段
/// （备注、分类、扣款账户、商户）。`amount_cents` / `total_amount_cents` 为兼容哨兵：
/// 请求一旦携带即被后端显式拒绝——改价 = 取消旧计划 + 新建，不做「改价对未来生效」。
/// `merchant_id` 与其他字段同款**全量替换**语义：未携带字段在调用方补齐当前值。
#[derive(Debug, Deserialize)]
pub struct UpdateSubscriptionInput {
    pub id: String,
    pub account_id: String,
    pub category_id: Option<String>,
    pub note: Option<String>,
    /// 商户引用（issue #190 / ADR-0028）：可改商户，编辑只影响未来期次
    /// （期次执行时从计划扩展表读取 merchant_id）。
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
    /// 商户 id（installment/subscription 可携带；scheduled_transfer 恒为 None）。
    pub merchant_id: Option<String>,
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
    /// 失败期次（issue #205）：期次详情弹窗「重试」的数据源。
    pub failed_occurrences: Vec<ScheduledTransactionOccurrence>,
    /// 已完成期次列表（issue #205）：期次详情弹窗展示每期执行状态；
    /// `completed_occurrences` 计数字段为既有契约，保留不动。
    pub completed_occurrence_list: Vec<ScheduledTransactionOccurrence>,
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
