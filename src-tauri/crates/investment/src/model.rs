use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

use super::backfill::TrendBackfillStatus;
use super::channel::PriceChannel;
use super::market::Market;
use ledger_infra::closed_set::closed_set;
use ledger_infra::db::query::FromRow;
use ledger_transaction::amount::TransactionKind;

pub const MANUAL_SOURCE: &str = "manual";

closed_set! {
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentType {
    Stock => "stock",
    Fund => "fund",
    Bond => "bond",
    Etf => "etf",
    Other => "other",
}
err_label = "金融工具类型",
err_code = "instrument.type-unknown",
}

impl Serialize for InstrumentType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InstrumentType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        InstrumentType::parse(&s).map_err(serde::de::Error::custom)
    }
}

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
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for InstrumentType {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        InstrumentType::parse(value.as_str()?).map_err(|e| FromSqlError::Other(Box::new(e)))
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
    pub source: String,
    pub price_cents: Option<i64>,
    pub invested: bool,
    pub price_channel: PriceChannel,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstrumentListFilter {
    pub search: Option<String>,
    pub market: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<InstrumentType>,
    pub only_invested: Option<bool>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub anchor_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct InstrumentListResult {
    pub items: Vec<Instrument>,
    pub total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_page: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct AddFundResult {
    pub instrument_id: String,
    pub symbol: String,
    pub name: String,
    pub fund_class: String,
    pub nav_cents: Option<i64>,
    pub nav_date: Option<String>,
    pub price_written: bool,
}

#[derive(Debug, Serialize)]
pub struct AddStockInstrumentResult {
    pub instrument_id: String,
    pub symbol: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    pub market: String,
    pub currency_code: String,
    pub price_cents: Option<i64>,
    pub price_date: Option<String>,
    pub price_written: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct TransactionTrade {
    pub instrument_id: String,
    pub symbol: String,
    pub instrument_name: Option<String>,
    pub instrument_type: String,
    pub quantity: f64,
    pub price_cents: i64,
    pub fee_cents: Option<i64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TransactionConvert {
    pub out_instrument_id: String,
    pub out_symbol: String,
    pub out_instrument_name: Option<String>,
    pub out_quantity: f64,
    pub out_amount_cents: i64,
    pub in_instrument_id: String,
    pub in_symbol: String,
    pub in_instrument_name: Option<String>,
    pub in_quantity: f64,
    pub in_amount_cents: i64,
    pub fee_cents: i64,
    pub carried_cost_cents: i64,
    pub currency_code: String,
}

impl FromRow for TransactionConvert {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(TransactionConvert {
            out_instrument_id: row.get(0)?,
            out_symbol: row.get(1)?,
            out_instrument_name: row.get(2)?,
            out_quantity: row.get(3)?,
            out_amount_cents: row.get(4)?,
            in_instrument_id: row.get(5)?,
            in_symbol: row.get(6)?,
            in_instrument_name: row.get(7)?,
            in_quantity: row.get(8)?,
            in_amount_cents: row.get(9)?,
            fee_cents: row.get(10)?,
            carried_cost_cents: row.get(11)?,
            currency_code: row.get(12)?,
        })
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct TransactionSplit {
    pub instrument_id: String,
    pub symbol: String,
    pub instrument_name: Option<String>,
    pub quantity: f64,
}

impl FromRow for TransactionSplit {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(TransactionSplit {
            instrument_id: row.get(0)?,
            symbol: row.get(1)?,
            instrument_name: row.get(2)?,
            quantity: row.get(3)?,
        })
    }
}

impl FromRow for TransactionTrade {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(TransactionTrade {
            instrument_id: row.get(0)?,
            symbol: row.get(1)?,
            instrument_name: row.get(2)?,
            instrument_type: row.get(3)?,
            quantity: row.get(4)?,
            price_cents: row.get(5)?,
            fee_cents: row.get(6)?,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InvestmentTransactionListFilter {
    pub account_id: Option<String>,
    pub instrument_id: Option<String>,
    pub kinds: Option<Vec<TransactionKind>>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct InvestmentTransactionListResult {
    pub items: Vec<InvestmentTransactionRow>,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct InvestmentTransactionRow {
    pub id: String,
    pub date: String,
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub account_id: String,
    pub funding_account_id: Option<String>,
    pub instrument_id: String,
    pub symbol: String,
    pub instrument_name: Option<String>,
    pub instrument_type: String,
    pub trade: Option<InvestmentTradeFields>,
    pub convert: Option<InvestmentConvertFields>,
    pub split: Option<InvestmentSplitFields>,
    pub note: Option<String>,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub source_order_no: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InvestmentTradeFields {
    pub quantity: f64,
    pub price_cents: i64,
    pub fee_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct InvestmentConvertFields {
    pub quantity: f64,
    pub to_instrument_id: String,
    pub to_symbol: String,
    pub to_quantity: f64,
    pub out_amount_cents: i64,
    pub in_amount_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct InvestmentSplitFields {
    pub delta_quantity: f64,
}

impl FromRow for InvestmentTransactionRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let kind: TransactionKind = row.get(1)?;
        let (trade, convert, split) = match kind {
            TransactionKind::Buy | TransactionKind::Sell => (
                Some(InvestmentTradeFields {
                    quantity: require_payload_field(row, 10, kind, "quantity")?,
                    price_cents: require_payload_field(row, 11, kind, "price_cents")?,
                    fee_cents: require_payload_field(row, 12, kind, "fee_cents")?,
                }),
                None,
                None,
            ),
            TransactionKind::Convert => (
                None,
                Some(InvestmentConvertFields {
                    quantity: require_payload_field(row, 10, kind, "quantity")?,
                    to_instrument_id: require_payload_field(row, 13, kind, "to_instrument_id")?,
                    to_symbol: require_payload_field(row, 14, kind, "to_symbol")?,
                    to_quantity: require_payload_field(row, 15, kind, "to_quantity")?,
                    out_amount_cents: require_payload_field(row, 16, kind, "out_amount_cents")?,
                    in_amount_cents: require_payload_field(row, 17, kind, "in_amount_cents")?,
                }),
                None,
            ),
            TransactionKind::Split => (
                None,
                None,
                Some(InvestmentSplitFields {
                    delta_quantity: require_payload_field(row, 10, kind, "quantity")?,
                }),
            ),
            TransactionKind::Dividend
            | TransactionKind::Income
            | TransactionKind::Expense
            | TransactionKind::Transfer
            | TransactionKind::Refund => (None, None, None),
        };
        Ok(Self {
            id: row.get(0)?,
            kind,
            amount_cents: row.get(2)?,
            account_id: row.get(3)?,
            funding_account_id: row.get(4)?,
            date: row.get(5)?,
            instrument_id: row.get(6)?,
            symbol: row.get(7)?,
            instrument_name: row.get(8)?,
            instrument_type: row.get(9)?,
            trade,
            convert,
            split,
            note: row.get(18)?,
            currency_code: row.get(19)?,
            amount_native_cents: row.get(20)?,
            source_order_no: row.get(21)?,
        })
    }
}

fn require_payload_field<T: rusqlite::types::FromSql>(
    row: &rusqlite::Row,
    idx: usize,
    kind: TransactionKind,
    field: &str,
) -> rusqlite::Result<T> {
    let value: Option<T> = row.get(idx)?;
    value.ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            idx,
            rusqlite::types::Type::Null,
            format!(
                "投资明细行载荷不变量破坏：kind={} 的字段 {field} 为 NULL",
                kind.as_str()
            )
            .into(),
        )
    })
}

#[derive(Debug, Clone)]
pub struct InstrumentSourceDisplay {
    pub transaction_id: String,
    pub instrument_id: String,
    pub symbol: String,
    pub name: Option<String>,
}

impl InstrumentSourceDisplay {
    pub fn display_label(&self) -> String {
        match self.name.as_deref() {
            Some(name) if !name.is_empty() => format!("{} {}", self.symbol, name),
            _ => self.symbol.clone(),
        }
    }
}

impl FromRow for InstrumentSourceDisplay {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(InstrumentSourceDisplay {
            transaction_id: row.get(0)?,
            instrument_id: row.get(1)?,
            symbol: row.get(2)?,
            name: row.get(3)?,
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
    pub latest_nav_date: Option<String>,
    pub market_value_cents: Option<i64>,
    pub unrealized_pnl_cents: Option<i64>,
    pub native_market_value_cents: Option<i64>,
    pub native_unrealized_pnl_cents: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MarketPrice {
    pub id: String,
    pub instrument_id: String,
    pub price_cents: i64,
    pub currency_code: String,
    pub priced_at: String,
    pub nav_date: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct ManualPriceInput {
    pub instrument_id: String,
    pub date: String,
    pub price_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct ManualPriceResult {
    pub history_written: bool,
    pub current_price_written: bool,
}

impl ManualPriceResult {
    pub fn any_written(&self) -> bool {
        self.history_written || self.current_price_written
    }
}

#[derive(Debug, Serialize)]
pub struct RealizedPnlSummary {
    pub by_year: Vec<YearPnl>,
    pub by_account: Vec<AccountPnl>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CurrencyCumulativePnl {
    pub currency_code: String,
    pub cumulative_pnl_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CurrencyHoldingTotals {
    pub currency_code: String,
    pub market_value_cents: Option<i64>,
    pub unrealized_pnl_cents: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CumulativePnlNativeTotal {
    pub total_cents: i64,
    pub native_currency: String,
}

#[derive(Debug, Serialize)]
pub struct YearPnl {
    pub year: String,
    pub native_currency: String,
    pub realized_pnl_cents: Option<i64>,
    pub dividend_cents: Option<i64>,
    pub realized_gain_cents: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AccountPnl {
    pub account_id: String,
    pub account_name: String,
    pub native_currency: String,
    pub realized_pnl_cents: Option<i64>,
    pub dividend_cents: Option<i64>,
    pub realized_gain_cents: Option<i64>,
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
            latest_nav_date: row.get(8)?,
            market_value_cents: row.get(9)?,
            unrealized_pnl_cents: row.get(10)?,
            native_market_value_cents: None,
            native_unrealized_pnl_cents: None,
            updated_at: row.get(11)?,
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
            nav_date: row.get(5)?,
            source: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            version: row.get(9)?,
            device_id: row.get(10)?,
        })
    }
}

impl FromRow for CurrencyCumulativePnl {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(CurrencyCumulativePnl {
            currency_code: row.get(0)?,
            cumulative_pnl_cents: row.get(1)?,
        })
    }
}

impl FromRow for CurrencyHoldingTotals {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(CurrencyHoldingTotals {
            currency_code: row.get(0)?,
            market_value_cents: row.get(1)?,
            unrealized_pnl_cents: row.get(2)?,
        })
    }
}

impl FromRow for Instrument {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let kind: InstrumentType = row.get(2)?;
        let market: Market = row.get(5)?;
        let symbol: String = row.get(1)?;
        let constant_unit_price: Option<i64> = row.get(13)?;
        let price_channel =
            super::channel::derive_price_channel(kind, market, &symbol, constant_unit_price);
        Ok(Instrument {
            id: row.get(0)?,
            symbol,
            kind,
            name: row.get(3)?,
            currency_code: row.get(4)?,
            market: market.as_str().to_string(),
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            version: row.get(8)?,
            device_id: row.get(9)?,
            source: row.get(10)?,
            price_cents: row.get(11)?,
            invested: row.get(12)?,
            price_channel,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TrendRange {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PortfolioTrendPoint {
    pub date: String,
    pub market_value_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct PortfolioValueTrend {
    pub currency_code: String,
    pub points: Vec<PortfolioTrendPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backfill: Option<TrendBackfillStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FinancialFreedomOverview {
    pub ratio: f64,
    pub numerator_cents: i64,
    pub denominator_cents: i64,
    pub coverage_years: f64,
    pub native_currency: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvestmentOverview {
    pub native_currency: String,
    pub investable_assets_cents: i64,
    pub investment_cash_cents: i64,
    pub holdings_market_value_cents: i64,
    pub total_market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
    pub cumulative_pnl_cents: i64,
    pub missing_price_holding_count: i64,
    pub has_investment_account: bool,
}
