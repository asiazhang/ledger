//! 投资领域模型：金融工具、持仓、行情价、已实现盈亏、标的列表分页。

use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::db::query::FromRow;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentType {
    Stock,
    Fund,
    Bond,
    Etf,
    Other,
}

impl InstrumentType {
    /// 闭集全量清单（OpenAPI 枚举等消费；先例：`TransactionKind::ALL`）。
    pub const ALL: [InstrumentType; 5] = [
        InstrumentType::Stock,
        InstrumentType::Fund,
        InstrumentType::Bond,
        InstrumentType::Etf,
        InstrumentType::Other,
    ];
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

// OpenAPI（utoipa）：闭集枚举以小写字符串枚举值入文档，与 wire 格式一致
// （先例：`TransactionKind`，内联 schema，消费方字段直接嵌入、无需注册组件；
// 枚举值由 [`InstrumentType::ALL`] 驱动，变体增减单点同步）。
impl PartialSchema for InstrumentType {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(InstrumentType::ALL.map(|k| k.to_string())))
                .description(Some(
                    "金融工具类型（闭集，小写字符串，与 instruments.instrument_type 一致）",
                ))
                .build(),
        ))
    }
}

impl ToSchema for InstrumentType {}

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

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
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
    /// 是否持有该标的（有当前持仓批次 remaining_quantity > 0，派生自 security_lots）。
    pub invested: bool,
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

/// 标的列表查询过滤条件（服务端分页 + 搜索）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstrumentListFilter {
    /// 对 symbol / name 的大小写不敏感子串匹配。
    pub search: Option<String>,
    /// 交易市场精确匹配（sh / sz / hk / unknown）。
    pub market: Option<String>,
    /// 标的类型过滤（stock/fund/bond/etf/other）：同码异类型消歧用（issue #294）。
    #[serde(rename = "type")]
    pub kind: Option<InstrumentType>,
    /// 只看持仓标的：仅返回有当前持仓（remaining_quantity > 0）的标的。
    pub only_invested: Option<bool>,
    /// 页码，从 1 开始，默认 1。
    pub page: Option<usize>,
    /// 每页条数，默认 50，上限 500。
    pub page_size: Option<usize>,
}

/// 标的列表分页结果。
#[derive(Debug, Serialize, ToSchema)]
pub struct InstrumentListResult {
    pub items: Vec<Instrument>,
    /// 满足过滤条件的总条数（用于分页条）。
    pub total: i64,
}

/// 交易买卖明细（issue #180）：一笔 buy/sell 交易在 `security_transactions` 扩展表
/// 中的投影（核心 `transactions` 行不含投资字段，见 ADR-0003 核心表 + 扩展表），
/// 供投资表单编辑模式回填标的/数量/价格/费用。`symbol`/`instrument_name` 为
/// JOIN `instruments` 带出的展示字段，保证回填后标的选择框可直接显示标的而非裸 id。
#[derive(Debug, Serialize, Clone)]
pub struct TransactionTrade {
    pub instrument_id: String,
    pub symbol: String,
    pub instrument_name: Option<String>,
    pub quantity: f64,
    pub price_cents: i64,
    pub fee_cents: Option<i64>,
}

impl FromRow for TransactionTrade {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(TransactionTrade {
            instrument_id: row.get(0)?,
            symbol: row.get(1)?,
            instrument_name: row.get(2)?,
            quantity: row.get(3)?,
            price_cents: row.get(4)?,
            fee_cents: row.get(5)?,
        })
    }
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
            invested: row.get(11)?,
        })
    }
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

// ---------------------------------------------------------------------------
// 走势查询（issue #138 / spec #135 / ADR-0019：PortfolioValueTrend）
// ---------------------------------------------------------------------------

/// 走势查询区间：可选起止 ISO 8601 日期，`None` 表示该侧不设界。
/// 前端预设区间（1 月 / 3 月 / 1 年 / 全部）换算成起止日期传入。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TrendRange {
    /// 起始日期（含），ISO 8601。
    pub start_date: Option<String>,
    /// 截止日期（含），ISO 8601。
    pub end_date: Option<String>,
}

/// 单标的走势采样点：周采样交易日 + 收盘价（报价币种整数分）。
#[derive(Debug, Serialize)]
pub struct PriceTrendPoint {
    /// 周采样交易日（该周最后一个有报价交易日），ISO 8601 日期。
    pub date: String,
    /// 收盘价（分，报价币种）。
    pub price_cents: i64,
    /// 报价币种（港股 HKD、沪深 CNY）。
    pub currency_code: String,
}

/// 单标的走势：区间裁剪后的周采样点序列（PriceHistory 直出，从首个有效点开始）。
#[derive(Debug, Serialize)]
pub struct InstrumentPriceTrend {
    pub instrument_id: String,
    pub points: Vec<PriceTrendPoint>,
}

/// 组合走势采样点：该周各持仓标的「持有数量 × 周线价格」折算到本位币后的合计。
#[derive(Debug, Serialize)]
pub struct PortfolioTrendPoint {
    /// 所属 ISO 周的周一（周点 x 坐标，按周连续、缺口连点跨越），ISO 8601 日期。
    pub date: String,
    /// 该周组合总市值（分，本位币）。
    pub market_value_cents: i64,
}

/// 投资资产走势（PortfolioValueTrend）：组合市值周点曲线。
/// `points` 为空即无任何历史数据的空态（前端据此渲染引导文案）。
#[derive(Debug, Serialize)]
pub struct PortfolioValueTrend {
    /// 折算基准（本位币）。
    pub currency_code: String,
    pub points: Vec<PortfolioTrendPoint>,
}
