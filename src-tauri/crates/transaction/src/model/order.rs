use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct OrderAccountContribution {
    pub account_id: String,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TransactionOrderSummary {
    pub source_order_no: String,
    pub row_count: i64,
    pub total_amount_cents: i64,
    pub currency_code: Option<String>,
    pub accounts: Vec<OrderAccountContribution>,
    pub items: Vec<crate::model::Transaction>,
}
