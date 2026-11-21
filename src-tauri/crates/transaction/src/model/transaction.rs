use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::amount::{FxRateSource, TransactionKind};
use ledger_infra::db::query::FromRow;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Transaction {
    pub id: String,
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    pub funding_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub fx_rate_used: Option<f64>,
    pub fx_rate_source: Option<FxRateSource>,
    pub note: Option<String>,
    pub source_order_no: Option<String>,
    pub date: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
    #[serde(default)]
    pub fundings: Vec<TransactionFunding>,
    #[serde(default)]
    pub purchases: Vec<TransactionPurchase>,
    pub source: Option<TransactionSource>,
    pub convert: Option<ConvertFields>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TransactionFunding {
    pub account_id: String,
    pub amount_cents: i64,
    pub label: Option<String>,
    pub derived: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TransactionPurchase {
    pub name: String,
    pub quantity: i64,
    pub category_id: Option<String>,
    pub unit_price_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConvertFields {
    pub to_instrument_id: String,
    pub to_symbol: String,
    pub to_quantity: f64,
    pub out_amount_cents: i64,
    pub in_amount_cents: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum TransactionSourceKind {
    InstallmentPlan,
    Subscription,
    ScheduledTransfer,
    Policy,
    Item,
    Instrument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum TransactionSourceStatus {
    Cancelled,
    Disposed,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TransactionSource {
    pub kind: TransactionSourceKind,
    pub entity_id: String,
    pub display_name: String,
    pub status: Option<TransactionSourceStatus>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct TransactionSearchResult {
    pub items: Vec<Transaction>,
    pub total: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TransactionListResult {
    pub items: Vec<Transaction>,
    pub total: i64,
}

pub(crate) const ROW_COLUMNS: &[&str] = &[
    "id",
    "kind",
    "amount_cents",
    "currency_code",
    "amount_native_cents",
    "account_id",
    "to_account_id",
    "funding_account_id",
    "category_id",
    "refund_of_transaction_id",
    "note",
    "date",
    "created_at",
    "updated_at",
    "version",
    "device_id",
    "is_deleted",
    "merchant_id",
    "policy_id",
    "fx_rate_used",
    "fx_rate_source",
    "source_order_no",
];

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
            funding_account_id: row.get(7)?,
            category_id: row.get(8)?,
            refund_of_transaction_id: row.get(9)?,
            note: row.get(10)?,
            date: row.get(11)?,
            created_at: row.get(12)?,
            updated_at: row.get(13)?,
            version: row.get(14)?,
            device_id: row.get(15)?,
            is_deleted: row.get::<_, i64>(16)? != 0,
            merchant_id: row.get(17)?,
            policy_id: row.get(18)?,
            fx_rate_used: row.get(19)?,
            fx_rate_source: row.get(20)?,
            source_order_no: row.get(21)?,
            fundings: Vec::new(),
            purchases: Vec::new(),
            source: None,
            convert: None,
        })
    }
}
