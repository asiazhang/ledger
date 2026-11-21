use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::amount::TransactionKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecurityOrigin {
    #[default]
    Trade,
    Opening,
}

impl SecurityOrigin {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Trade => "trade",
            Self::Opening => "opening",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TransactionFundingInput {
    pub account_id: String,
    pub amount_cents: i64,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TransactionPurchaseInput {
    pub name: String,
    pub quantity: i64,
    pub category_id: Option<String>,
    pub unit_price_cents: Option<i64>,
}
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TransactionInput {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    pub funding_account_id: Option<String>,
    #[serde(default)]
    pub funding: Vec<TransactionFundingInput>,
    #[serde(default)]
    pub purchases: Vec<TransactionPurchaseInput>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub merchant_name: Option<String>,
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub source_order_no: Option<String>,
    pub date: String,
    pub instrument_id: Option<String>,
    pub quantity: Option<f64>,
    pub price_cents: Option<i64>,
    pub fee_cents: Option<i64>,
    pub to_instrument_id: Option<String>,
    pub to_quantity: Option<f64>,
    pub out_amount_cents: Option<i64>,
    pub in_amount_cents: Option<i64>,
    pub origin: Option<SecurityOrigin>,
    pub fx_rate: Option<f64>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateTransactionInput {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    pub funding_account_id: Option<String>,
    #[serde(default)]
    pub funding: Vec<TransactionFundingInput>,
    #[serde(default)]
    pub purchases: Vec<TransactionPurchaseInput>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub merchant_name: Option<String>,
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub source_order_no: Option<String>,
    pub date: String,
    pub instrument_id: Option<String>,
    pub quantity: Option<f64>,
    pub price_cents: Option<i64>,
    pub fee_cents: Option<i64>,
    pub to_instrument_id: Option<String>,
    pub to_quantity: Option<f64>,
    pub out_amount_cents: Option<i64>,
    pub in_amount_cents: Option<i64>,
    pub origin: Option<SecurityOrigin>,
    pub fx_rate: Option<f64>,
}

impl From<UpdateTransactionInput> for TransactionInput {
    fn from(u: UpdateTransactionInput) -> Self {
        TransactionInput {
            kind: u.kind,
            amount_cents: u.amount_cents,
            currency_code: u.currency_code,
            account_id: u.account_id,
            to_account_id: u.to_account_id,
            funding_account_id: u.funding_account_id,
            funding: u.funding,
            purchases: u.purchases,
            category_id: u.category_id,
            merchant_id: u.merchant_id,
            merchant_name: u.merchant_name,
            policy_id: u.policy_id,
            refund_of_transaction_id: u.refund_of_transaction_id,
            note: u.note,
            source_order_no: u.source_order_no,
            date: u.date,
            instrument_id: u.instrument_id,
            quantity: u.quantity,
            price_cents: u.price_cents,
            fee_cents: u.fee_cents,
            to_instrument_id: u.to_instrument_id,
            to_quantity: u.to_quantity,
            out_amount_cents: u.out_amount_cents,
            in_amount_cents: u.in_amount_cents,
            origin: u.origin,
            fx_rate: u.fx_rate,
            idempotency_key: None,
        }
    }
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
