use serde::{Deserialize, Serialize};

use crate::model::SecurityOrigin;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvertCommandFields {
    pub instrument_id: String,
    pub quantity: f64,
    pub to_instrument_id: String,
    pub to_quantity: f64,
    pub out_amount_cents: i64,
    pub in_amount_cents: i64,
    pub fee_cents: i64,
    pub carried_cost_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitCommandFields {
    pub instrument_id: String,
    pub delta_quantity: f64,
    pub final_quantity: f64,
    pub total_cost_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestmentCommandFields {
    pub instrument_id: String,
    pub quantity: f64,
    pub price_cents: i64,
    pub fee_cents: i64,
    pub cost_per_unit_cents: Option<i64>,
    pub origin: Option<SecurityOrigin>,
}
