use std::fmt;
use std::str::FromStr;

use ledger_infra::db::query::FromRow;
use ledger_infra::error::AppError;
use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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
    #[doc = "黑洞账户标志：对用户侧列表/余额/下拉选择器隐藏，但交易仍参与交易列表与报表。"]
    pub is_hidden: bool,
    #[doc = "归档收纳态：`true` = 已归档，只收窄账户页主表与编辑选择面两处呈现；余额、余额合计与净资产照旧计入，读面不过滤。"]
    pub is_archived: bool,
    #[doc = "信用额度（整数分，仅信用卡；`None` = 未设置）。档案字段，不参与余额与净资产。"]
    pub credit_limit_cents: Option<i64>,
    #[doc = "账单日（1–31 的每月第 N 日声明值，仅信用卡；`None` = 未设置）。"]
    pub statement_day: Option<i64>,
    #[doc = "还款日（1–31 的每月第 N 日声明值，仅信用卡；`None` = 未设置）。"]
    pub due_day: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AccountInput {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: AccountType,
    pub currency_code: String,
    pub initial_balance_cents: Option<i64>,
    #[doc = "信用卡档案字段（仅 `credit` 账户；缺省/`null` = 未设置，三个字段彼此独立）。"]
    pub credit_limit_cents: Option<i64>,
    pub statement_day: Option<i64>,
    pub due_day: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "账户编辑入参（IPC `update_account` / HTTP `PUT /api/v1/accounts/{id}`）。`type` 不可改：账户类型参与 kind→符号矩阵（余额方向），改动会重写历史交易的余额归属（ADR-0026 同期决策，Q3）；`initial_balance_cents` 不在此改，归余额调整（见参考数据与设置域 BalanceAdjustment）。"
)]
pub struct AccountUpdateInput {
    pub name: Option<String>,
    #[doc = "仅无交易账户可改（有交易时改币种会使历史折算口径错乱，后端拒绝）。"]
    pub currency_code: Option<String>,
    #[doc = "信用卡档案字段三态：键缺席 = 不改、`null` = 清空、给值 = 落定该值（区分器 [`ledger_infra::serde_util::double_option`]：serde 默认把「键缺席」与「值为 `null`」折叠成同一 `None`，原理与用法见其模块文档）。"]
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub credit_limit_cents: Option<Option<i64>>,
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub statement_day: Option<Option<i64>>,
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub due_day: Option<Option<i64>>,
    #[doc = "可选归档键：缺席 = 不改，`true` / `false` = 落定归档 / 恢复（布尔无「清空」态，`null` 同缺席）。"]
    #[serde(default)]
    pub is_archived: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CreditTerms {
    pub credit_limit_cents: Option<i64>,
    pub statement_day: Option<i64>,
    pub due_day: Option<i64>,
}

impl CreditTerms {
    pub fn new(
        credit_limit_cents: Option<i64>,
        statement_day: Option<i64>,
        due_day: Option<i64>,
    ) -> Self {
        Self {
            credit_limit_cents,
            statement_day,
            due_day,
        }
    }

    pub fn is_unset(&self) -> bool {
        self.credit_limit_cents.is_none() && self.statement_day.is_none() && self.due_day.is_none()
    }

    pub fn validate_for(&self, kind: AccountType) -> Result<(), AppError> {
        if self.is_unset() {
            return Ok(());
        }
        if kind != AccountType::Credit {
            return Err(AppError::coded(
                "account.credit-attribute-not-applicable",
                "信用额度、账单日与还款日仅信用卡账户可填写",
            ));
        }
        if self.credit_limit_cents.is_some_and(|limit| limit <= 0) {
            return Err(AppError::coded(
                "account.credit-limit-invalid",
                "信用额度必须大于 0",
            ));
        }
        for (day, code, label) in [
            (
                self.statement_day,
                "account.statement-day-invalid",
                "账单日",
            ),
            (self.due_day, "account.due-day-invalid", "还款日"),
        ] {
            if let Some(day) = day.filter(|d| !(1..=31).contains(d)) {
                let value = day.to_string();
                return Err(AppError::codedp(
                    code,
                    format!("{label}必须在 1 到 31 之间: {value}"),
                    &[value.as_str()],
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountBalanceAdjustInput {
    pub target_balance_cents: i64,
    pub date: String,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AccountBalance {
    pub account: Account,
    pub balance_cents: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BalanceCacheDrift {
    pub account_id: String,
    pub account_name: String,
    pub cached_cents: Option<i64>,
    pub actual_cents: i64,
}

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
            is_archived: row.get::<_, i64>(11)? != 0,
            credit_limit_cents: row.get(12)?,
            statement_day: row.get(13)?,
            due_day: row.get(14)?,
        })
    }
}
