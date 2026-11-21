use serde::{Deserialize, Serialize};

use crate::amount::{FxRateSource, TransactionKind};

use crate::model::{TransactionFundingInput, TransactionPurchaseInput};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedTransaction {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    #[serde(default)]
    pub fx_rate_used: Option<f64>,
    #[serde(default)]
    pub fx_rate_source: Option<FxRateSource>,
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    pub funding_account_id: Option<String>,
    #[serde(default)]
    pub funding: Vec<TransactionFundingInput>,
    #[serde(default)]
    pub purchases: Vec<TransactionPurchaseInput>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    #[serde(default)]
    pub source_order_no: Option<String>,
    pub date: String,
}
