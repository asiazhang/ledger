use cucumber::World;

#[macro_use]
#[path = "e2e/world.rs"]
mod world;

#[path = "e2e/accounts_steps.rs"]
mod accounts_steps;
#[path = "e2e/backup_steps.rs"]
mod backup_steps;
#[path = "e2e/budget_steps.rs"]
mod budget_steps;
#[path = "e2e/common.rs"]
mod common;
#[path = "e2e/dashboard_steps.rs"]
mod dashboard_steps;
#[path = "e2e/data_location_steps.rs"]
mod data_location_steps;
#[path = "e2e/financial_freedom_steps.rs"]
mod financial_freedom_steps;
#[path = "e2e/fund_trade_steps.rs"]
mod fund_trade_steps;
#[path = "e2e/instruments_steps.rs"]
mod instruments_steps;
#[path = "e2e/investment_migration_steps.rs"]
mod investment_migration_steps;
#[path = "e2e/investment_trend_steps.rs"]
mod investment_trend_steps;
#[path = "e2e/items_common.rs"]
mod items_common;
#[path = "e2e/items_cost_steps.rs"]
mod items_cost_steps;
#[path = "e2e/items_create_steps.rs"]
mod items_create_steps;
#[path = "e2e/items_dispose_steps.rs"]
mod items_dispose_steps;
#[path = "e2e/items_provenance_steps.rs"]
mod items_provenance_steps;
#[path = "e2e/items_update_steps.rs"]
mod items_update_steps;
#[path = "e2e/manual_quote_steps.rs"]
mod manual_quote_steps;
#[path = "e2e/merchants_steps.rs"]
mod merchants_steps;
#[path = "e2e/migration_steps.rs"]
mod migration_steps;
#[path = "e2e/policies_steps.rs"]
mod policies_steps;
#[path = "e2e/policy_agreement_steps.rs"]
mod policy_agreement_steps;
#[path = "e2e/policy_stats_steps.rs"]
mod policy_stats_steps;
#[path = "e2e/reports_steps.rs"]
mod reports_steps;
#[path = "e2e/scheduled_steps.rs"]
mod scheduled_steps;
#[path = "e2e/search_steps.rs"]
mod search_steps;
#[path = "e2e/transactions_edit_steps.rs"]
mod transactions_edit_steps;
#[path = "e2e/transactions_policy_steps.rs"]
mod transactions_policy_steps;
#[path = "e2e/transactions_query_steps.rs"]
mod transactions_query_steps;
#[path = "e2e/transactions_write_steps.rs"]
mod transactions_write_steps;

#[tokio::main]
async fn main() {
    world::LedgerWorld::run("tests/e2e/features").await;
}
