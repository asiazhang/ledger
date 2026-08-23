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

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Currency {
    pub code: String,
    pub name: String,
    pub symbol: String,
    pub decimal_places: i64,
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

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CategoryInput {
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CategoryUpdateInput {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub parent_id: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderItem {
    pub id: String,
    pub sort_order: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
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

/// 交易搜索分页结果（服务端分页）。
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct TransactionSearchResult {
    /// 匹配交易（当前页）。
    pub items: Vec<Transaction>,
    /// 命中总数（供「命中 N 条」与分页）。
    pub total: i64,
    /// 索引是否可能滞后：搜索重建队列非空（存在尚未消费的写入）时 true。
    /// 搜索为只读操作，不触发消费，故该值反映查询时刻的真实状态。
    pub stale: bool,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
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

/// 按 kind 校验并归一化后的一笔交易行字段（供创建与修改共用）。
///
/// 创建路径据此 INSERT、修改路径据此 UPDATE —— 校验与字段解析只做一次。
/// buy/sell 的持仓/卖出关联等副作用由调用方在落库时按其身份（新增或替换）另行执行。
#[derive(Debug, Clone)]
pub struct NormalizedTransaction {
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

#[derive(Debug, Serialize, ToSchema)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateTransactionResult {
    pub success: bool,
    pub duplicate: bool,
    pub id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TransactionBatchInput {
    pub transactions: Vec<TransactionInput>,
    #[serde(default = "default_dedup")]
    pub dedup: bool,
}

fn default_dedup() -> bool {
    true
}

/// 交易列表查询过滤条件（服务端分页 + 过滤）。
///
/// 与 `InstrumentListFilter` 先例对齐（`page_size` 下划线命名，serde 保持原样透传）。
/// 分页语义：`page` 从 1 起、缺省 1；`page_size` 缺省时返回全部（`total` 恒返回）；
/// `limit` 为独立的"取前 N 条"参数（仪表盘"最近 N 条"场景），传 `page_size` 时分页路径生效。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionListFilter {
    /// 起始日期（含），YYYY-MM-DD。
    pub from: Option<String>,
    /// 结束日期（含），YYYY-MM-DD。
    pub to: Option<String>,
    /// 按转出账户过滤。
    pub account_id: Option<String>,
    /// 交易类型过滤（income / expense / transfer / buy / sell / refund）。
    pub kind: Option<String>,
    /// 取前 N 条（仪表盘"最近 N 条"场景），与分页互斥：传 `page_size` 时分页路径生效。
    /// 沿用 SQLite 原生语义：`limit=0` 返回空，负值无上限。
    pub limit: Option<i64>,
    /// 页码，从 1 开始，默认 1。
    pub page: Option<usize>,
    /// 每页条数，缺省返回全部（total 恒返回）；小于 1 按 1 处理。
    pub page_size: Option<usize>,
}

/// 交易列表分页结果。
#[derive(Debug, Serialize, ToSchema)]
pub struct TransactionListResult {
    pub items: Vec<Transaction>,
    /// 满足过滤条件的未删除交易总数（用于分页条）。
    pub total: i64,
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
    pub market: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    /// 最新市场价格（分），同步来源；无行情时为空。
    pub price_cents: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct InstrumentInput {
    pub symbol: String,
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    pub name: Option<String>,
    pub currency_code: String,
    pub market: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgress {
    pub current: usize,
    pub total: usize,
    pub market: String,
    pub done: bool,
    pub total_inserted: usize,
    pub total_updated: usize,
    pub error: Option<String>,
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
            is_hidden: row.get::<_, i64>(10)? != 0,
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
            sort_order: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            version: row.get(8)?,
            device_id: row.get(9)?,
            is_deleted: row.get::<_, i64>(10)? != 0,
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
            market: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            version: row.get(8)?,
            device_id: row.get(9)?,
            price_cents: row.get(10)?,
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

/// 标的列表查询过滤条件（服务端分页 + 搜索）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstrumentListFilter {
    /// 对 symbol / name 的大小写不敏感子串匹配。
    pub search: Option<String>,
    /// 交易市场精确匹配（sh / sz / hk / unknown）。
    pub market: Option<String>,
    /// 页码，从 1 开始，默认 1。
    pub page: Option<usize>,
    /// 每页条数，默认 50，上限 500。
    pub page_size: Option<usize>,
}

/// 标的列表分页结果。
#[derive(Debug, Serialize)]
pub struct InstrumentListResult {
    pub items: Vec<Instrument>,
    /// 满足过滤条件的总条数（用于分页条）。
    pub total: i64,
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
