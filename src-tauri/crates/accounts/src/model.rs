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
    pub is_hidden: bool,
    pub is_archived: bool,
    pub credit_limit_cents: Option<i64>,
    pub statement_day: Option<i64>,
    pub due_day: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AccountInput {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: AccountType,
    pub currency_code: String,
    pub initial_balance_cents: Option<i64>,
    pub credit_limit_cents: Option<i64>,
    pub statement_day: Option<i64>,
    pub due_day: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AccountUpdateInput {
    pub name: Option<String>,
    pub currency_code: Option<String>,
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub credit_limit_cents: Option<Option<i64>>,
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub statement_day: Option<Option<i64>>,
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub due_day: Option<Option<i64>>,
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
