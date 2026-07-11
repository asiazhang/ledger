use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
            _ => Err(AppError::Invalid(format!("未知账户类型: {s}"))),
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
pub struct Currency {
    pub code: String,
    pub name: String,
    pub symbol: String,
    pub decimal_places: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
}

#[derive(Debug, Deserialize)]
pub struct AccountInput {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: AccountType,
    pub currency_code: String,
    pub initial_balance_cents: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, Deserialize)]
pub struct CategoryInput {
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transaction {
    pub id: String,
    pub kind: String,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub account_id: String,
    pub to_account_id: Option<String>,
    pub category_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, Deserialize)]
pub struct TransactionInput {
    pub kind: String,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: String,
    pub to_account_id: Option<String>,
    pub category_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub instrument_id: Option<String>,
    pub quantity: Option<f64>,
    pub price_cents: Option<i64>,
    pub fee_cents: Option<i64>,
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

#[derive(Debug, Serialize)]
pub struct AccountBalance {
    pub account: Account,
    pub balance_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct MonthlySummary {
    pub month: String,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub refund_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct CategoryShare {
    pub category_id: String,
    pub category_name: String,
    pub amount_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct BudgetProgress {
    pub budget: Budget,
    pub category_name: String,
    pub spent_cents: i64,
    pub over_budget: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImportedRow {
    pub date: String,
    pub amount_cents: i64,
    pub note: String,
    pub category_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExchangeRate {
    pub id: String,
    pub base_code: String,
    pub quote_code: String,
    pub rate: f64,
    pub priced_at: String,
    pub source: Option<String>,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeRateInput {
    pub base_code: String,
    pub quote_code: String,
    pub rate: f64,
    pub priced_at: String,
    pub source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MarketPrice {
    pub id: String,
    pub instrument_id: String,
    pub price_cents: i64,
    pub currency_code: String,
    pub priced_at: String,
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
pub struct MarketPriceInput {
    pub instrument_id: String,
    pub price_cents: i64,
    pub currency_code: String,
    pub priced_at: String,
    pub source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Holding {
    pub id: String,
    pub account_id: String,
    pub instrument_id: String,
    pub quantity: f64,
    pub cost_basis_cents: i64,
    pub cost_currency_code: String,
    pub latest_price_cents: Option<i64>,
    pub latest_price_currency_code: Option<String>,
    pub market_value_cents: Option<i64>,
    pub unrealized_pnl_cents: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Instrument {
    pub id: String,
    pub symbol: String,
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    pub name: Option<String>,
    pub currency_code: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
pub struct InstrumentInput {
    pub symbol: String,
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    pub name: Option<String>,
    pub currency_code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentType {
    Stock,
    Fund,
    Bond,
    Etf,
    Other,
}

impl fmt::Display for InstrumentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstrumentType::Stock => write!(f, "stock"),
            InstrumentType::Fund => write!(f, "fund"),
            InstrumentType::Bond => write!(f, "bond"),
            InstrumentType::Etf => write!(f, "etf"),
            InstrumentType::Other => write!(f, "other"),
        }
    }
}

impl FromStr for InstrumentType {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stock" => Ok(InstrumentType::Stock),
            "fund" => Ok(InstrumentType::Fund),
            "bond" => Ok(InstrumentType::Bond),
            "etf" => Ok(InstrumentType::Etf),
            "other" => Ok(InstrumentType::Other),
            _ => Err(AppError::Invalid(format!("未知金融工具类型: {s}"))),
        }
    }
}

impl ToSql for InstrumentType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for InstrumentType {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        value
            .as_str()?
            .parse()
            .map_err(|e: AppError| FromSqlError::Other(Box::new(e)))
    }
}
