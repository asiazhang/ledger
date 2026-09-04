//! 账户领域模型（#419 随域归位）：账户类型枚举、账户实体与入参、账户余额读模型 DTO。
//!
//! 自全局模型目录迁入本域（#417 归属原则：实体归属优先于消费方分布），
//! 消费方经 `accounts` 域路径逐类型显式 import。余额计算引擎与余额读查询
//! 留驻基础设施 `db::balance`（#404 既定裁决不翻案），改经域路径消费本域
//! 类型——「基础设施→域类型消费」允许边自此正式落地（ADR-0059 决策 5）。

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::query::FromRow;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Cash,
    Bank,
    Credit,
    Ewallet,
    Investment,
    Debt,
    Receivable,
    Other,
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountType::Cash => write!(f, "cash"),
            AccountType::Bank => write!(f, "bank"),
            AccountType::Credit => write!(f, "credit"),
            AccountType::Ewallet => write!(f, "ewallet"),
            AccountType::Investment => write!(f, "investment"),
            AccountType::Debt => write!(f, "debt"),
            AccountType::Receivable => write!(f, "receivable"),
            AccountType::Other => write!(f, "other"),
        }
    }
}

impl FromStr for AccountType {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cash" => Ok(AccountType::Cash),
            "bank" => Ok(AccountType::Bank),
            "credit" => Ok(AccountType::Credit),
            "ewallet" => Ok(AccountType::Ewallet),
            "investment" => Ok(AccountType::Investment),
            "debt" => Ok(AccountType::Debt),
            "receivable" => Ok(AccountType::Receivable),
            "other" => Ok(AccountType::Other),
            _ => Err(AppError::codedp(
                "account.type-unknown",
                format!("未知账户类型: {s}"),
                &[s],
            )),
        }
    }
}

impl ToSql for AccountType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for AccountType {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        value
            .as_str()?
            .parse()
            .map_err(|e: AppError| FromSqlError::Other(Box::new(e)))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Account {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: AccountType,
    pub currency_code: String,
    pub initial_balance_cents: i64,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
    /// 黑洞账户标志：对用户侧列表/余额/下拉选择器隐藏，但交易仍参与交易列表与报表。
    pub is_hidden: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AccountInput {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: AccountType,
    pub currency_code: String,
    pub initial_balance_cents: Option<i64>,
}

/// 账户编辑入参（IPC `update_account` / HTTP `PUT /api/v1/accounts/{id}`）。
/// `type` 不可改：账户类型参与 kind→符号矩阵（余额方向），改动会重写历史交易
/// 的余额归属（ADR-0026 同期决策，Q3）；`initial_balance_cents` 不在此改，
/// 归余额调整（见参考数据与设置域 BalanceAdjustment）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct AccountUpdateInput {
    pub name: Option<String>,
    /// 仅无交易账户可改（有交易时改币种会使历史折算口径错乱，后端拒绝）。
    pub currency_code: Option<String>,
}

/// 余额调整入参（IPC `adjust_account_balance`）：把余额校准到目标值，
/// 机制为生成一笔与黑洞账户的转账（ADR-0026）。
#[derive(Debug, Deserialize)]
pub struct AccountBalanceAdjustInput {
    pub target_balance_cents: i64,
    /// 调整交易日期（YYYY-MM-DD，对账常补记过去日期）。
    pub date: String,
    /// 调整交易备注；缺省后端补「余额调整」。
    pub note: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountBalance {
    pub account: Account,
    pub balance_cents: i64,
}

/// 余额缓存审计差异行（issue #491 / ADR-0067）：缓存缺失记 None（回填前）。
#[derive(Debug, Serialize, ToSchema)]
pub struct BalanceCacheDrift {
    pub account_id: String,
    pub account_name: String,
    pub cached_cents: Option<i64>,
    pub actual_cents: i64,
}

/// 余额缓存审计报告（issue #491 / ADR-0067）：修复已完成后的差异快照。
#[derive(Debug, Serialize, ToSchema)]
pub struct BalanceCacheAudit {
    pub accounts_checked: usize,
    pub drifts: Vec<BalanceCacheDrift>,
    pub repaired: bool,
}

impl FromRow for Account {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Account {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            currency_code: row.get(3)?,
            initial_balance_cents: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            version: row.get(7)?,
            device_id: row.get(8)?,
            is_deleted: row.get::<_, i64>(9)? != 0,
            is_hidden: row.get::<_, i64>(10)? != 0,
        })
    }
}
