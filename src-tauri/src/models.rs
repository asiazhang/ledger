use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

use crate::db::query::FromRow;
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
    pub sort_order: i64,
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
    pub icon: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CategoryUpdateInput {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub parent_id: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderItem {
    pub id: String,
    pub sort_order: i64,
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

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Serialize)]
pub struct CreateTransactionResult {
    pub success: bool,
    pub id: Option<String>,
    pub error: Option<String>,
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

// ---------------------------------------------------------------------------
// FromRow implementations (for db::query helpers)
// ---------------------------------------------------------------------------

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
        })
    }
}

impl FromRow for Category {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Category {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            parent_id: row.get(3)?,
            icon: row.get(4)?,
            color: row.get(5)?,
            sort_order: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            version: row.get(9)?,
            device_id: row.get(10)?,
            is_deleted: row.get::<_, i64>(11)? != 0,
        })
    }
}

impl FromRow for Currency {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Currency {
            code: row.get(0)?,
            name: row.get(1)?,
            symbol: row.get(2)?,
            decimal_places: row.get(3)?,
        })
    }
}

impl FromRow for Transaction {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Transaction {
            id: row.get(0)?,
            kind: row.get(1)?,
            amount_cents: row.get(2)?,
            currency_code: row.get(3)?,
            amount_native_cents: row.get(4)?,
            account_id: row.get(5)?,
            to_account_id: row.get(6)?,
            category_id: row.get(7)?,
            refund_of_transaction_id: row.get(8)?,
            note: row.get(9)?,
            date: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            version: row.get(13)?,
            device_id: row.get(14)?,
            is_deleted: row.get::<_, i64>(15)? != 0,
        })
    }
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

impl FromRow for Holding {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Holding {
            id: row.get(0)?,
            account_id: row.get(1)?,
            instrument_id: row.get(2)?,
            quantity: row.get(3)?,
            cost_basis_cents: row.get(4)?,
            cost_currency_code: row.get(5)?,
            latest_price_cents: row.get(6)?,
            latest_price_currency_code: row.get(7)?,
            market_value_cents: row.get(8)?,
            unrealized_pnl_cents: row.get(9)?,
            updated_at: row.get(10)?,
        })
    }
}

impl FromRow for ExchangeRate {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ExchangeRate {
            id: row.get(0)?,
            base_code: row.get(1)?,
            quote_code: row.get(2)?,
            rate: row.get(3)?,
            priced_at: row.get(4)?,
            source: row.get(5)?,
            updated_at: row.get(6)?,
            version: row.get(7)?,
            device_id: row.get(8)?,
        })
    }
}

impl FromRow for MarketPrice {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(MarketPrice {
            id: row.get(0)?,
            instrument_id: row.get(1)?,
            price_cents: row.get(2)?,
            currency_code: row.get(3)?,
            priced_at: row.get(4)?,
            source: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            version: row.get(8)?,
            device_id: row.get(9)?,
        })
    }
}

impl FromRow for YearPnl {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(YearPnl {
            year: row.get(0)?,
            realized_pnl_cents: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
        })
    }
}

impl FromRow for AccountPnl {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(AccountPnl {
            account_id: row.get(0)?,
            account_name: row.get(1)?,
            realized_pnl_cents: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    }
}

impl FromRow for InstrumentPnl {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(InstrumentPnl {
            instrument_id: row.get(0)?,
            symbol: row.get(1)?,
            name: row.get(2)?,
            realized_pnl_cents: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
        })
    }
}

impl FromRow for Instrument {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Instrument {
            id: row.get(0)?,
            symbol: row.get(1)?,
            kind: row.get(2)?,
            name: row.get(3)?,
            currency_code: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            version: row.get(7)?,
            device_id: row.get(8)?,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct RealizedPnlSummary {
    pub total_realized_pnl_cents: i64,
    pub by_year: Vec<YearPnl>,
    pub by_account: Vec<AccountPnl>,
    pub by_instrument: Vec<InstrumentPnl>,
    pub details: Vec<PnlDetail>,
}

#[derive(Debug, Serialize)]
pub struct YearPnl {
    pub year: String,
    pub realized_pnl_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct AccountPnl {
    pub account_id: String,
    pub account_name: String,
    pub realized_pnl_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct InstrumentPnl {
    pub instrument_id: String,
    pub symbol: String,
    pub name: Option<String>,
    pub realized_pnl_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct PnlDetail {
    pub id: String,
    pub sell_date: String,
    pub account_id: String,
    pub account_name: String,
    pub instrument_id: String,
    pub instrument_symbol: String,
    pub instrument_name: Option<String>,
    pub quantity: f64,
    pub cost_per_unit_cents: i64,
    pub realized_pnl_cents: i64,
    pub currency_code: String,
}

#[derive(Debug, Deserialize)]
pub struct PnlFilter {
    pub account_id: Option<String>,
    pub instrument_id: Option<String>,
}

impl FromRow for PnlDetail {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PnlDetail {
            id: row.get(0)?,
            sell_date: row.get(1)?,
            account_id: row.get(2)?,
            account_name: row.get(3)?,
            instrument_id: row.get(4)?,
            instrument_symbol: row.get(5)?,
            instrument_name: row.get(6)?,
            quantity: row.get(7)?,
            cost_per_unit_cents: row.get(8)?,
            realized_pnl_cents: row.get(9)?,
            currency_code: row.get(10)?,
        })
    }
}

impl FromRow for MonthlySummary {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(MonthlySummary {
            month: row.get::<_, String>(0)?,
            income_cents: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            expense_cents: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            refund_cents: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
        })
    }
}

impl FromRow for CategoryShare {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(CategoryShare {
            category_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            category_name: row.get(1)?,
            amount_cents: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    }
}
