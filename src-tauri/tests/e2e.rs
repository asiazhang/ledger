use cucumber::World;

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
#[path = "e2e/instruments_steps.rs"]
mod instruments_steps;
#[path = "e2e/items_steps.rs"]
mod items_steps;
#[path = "e2e/merchants_steps.rs"]
mod merchants_steps;
#[path = "e2e/migration_steps.rs"]
mod migration_steps;
#[path = "e2e/scheduled_steps.rs"]
mod scheduled_steps;
#[path = "e2e/search_steps.rs"]
mod search_steps;
#[path = "e2e/transactions_edit_steps.rs"]
mod transactions_edit_steps;
#[path = "e2e/transactions_query_steps.rs"]
mod transactions_query_steps;
#[path = "e2e/transactions_write_steps.rs"]
mod transactions_write_steps;
#[path = "e2e/world.rs"]
mod world;

#[tokio::main]
async fn main() {
    world::LedgerWorld::run("tests/e2e/features").await;
}
