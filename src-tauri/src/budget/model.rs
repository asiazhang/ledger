//! 预算领域模型（#420 随域归位）：预算周期枚举、预算实体与入参、进度。
//!
//! 自全局模型目录迁入本域（#417 归属原则），消费方经 `budget` 域路径
//! 逐类型显式 import。

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

use crate::db::query::FromRow;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetPeriod {
    Monthly,
    Yearly,
}

impl fmt::Display for BudgetPeriod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BudgetPeriod::Monthly => write!(f, "monthly"),
            BudgetPeriod::Yearly => write!(f, "yearly"),
        }
    }
}

impl FromStr for BudgetPeriod {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "monthly" => Ok(BudgetPeriod::Monthly),
            "yearly" => Ok(BudgetPeriod::Yearly),
            _ => Err(AppError::Invalid(format!("未知预算周期: {s}"))),
        }
    }
}

impl ToSql for BudgetPeriod {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for BudgetPeriod {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        value
            .as_str()?
            .parse()
            .map_err(|e: AppError| FromSqlError::Other(Box::new(e)))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Budget {
    pub id: String,
    pub category_id: String,
    pub period: BudgetPeriod,
    pub amount_cents: i64,
    pub start_date: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, Deserialize)]
pub struct BudgetInput {
    pub category_id: String,
    pub period: Option<BudgetPeriod>,
    pub amount_cents: i64,
    pub start_date: String,
}

/// 预算编辑入参（issue #184）：仅允许修改金额，分类/周期不可改（改法为删旧建新）。
#[derive(Debug, Deserialize)]
pub struct BudgetUpdateInput {
    pub amount_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct BudgetProgress {
    pub budget: Budget,
    pub category_name: String,
    pub spent_cents: i64,
    pub over_budget: bool,
}

impl FromRow for Budget {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Budget {
            id: row.get(0)?,
            category_id: row.get(1)?,
            period: row.get(2)?,
            amount_cents: row.get(3)?,
            start_date: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            version: row.get(7)?,
            device_id: row.get(8)?,
            is_deleted: row.get::<_, i64>(9)? != 0,
        })
    }
}

impl FromRow for BudgetProgress {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let amount_cents: i64 = row.get(3)?;
        let spent_cents: i64 = row.get(11)?;
        Ok(BudgetProgress {
            budget: Budget {
                id: row.get(0)?,
                category_id: row.get(1)?,
                period: row.get(2)?,
                amount_cents,
                start_date: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                version: row.get(7)?,
                device_id: row.get(8)?,
                is_deleted: row.get::<_, i64>(9)? != 0,
            },
            category_name: row
                .get::<_, Option<String>>(10)?
                .unwrap_or_else(|| "未分类".into()),
            spent_cents,
            over_budget: spent_cents > amount_cents,
        })
    }
}
