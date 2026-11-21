use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DashboardOverview {
    pub native_currency: String,
    pub net_worth_cents: i64,
    pub accounts_balance_cents: i64,
    pub holdings_market_value_cents: i64,
    pub physical_assets_value_cents: i64,
}
