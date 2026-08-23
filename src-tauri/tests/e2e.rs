use cucumber::World;

#[path = "e2e/accounts_steps.rs"]
mod accounts_steps;
#[path = "e2e/backup_steps.rs"]
mod backup_steps;
#[path = "e2e/common.rs"]
mod common;
#[path = "e2e/migration_steps.rs"]
mod migration_steps;
#[path = "e2e/search_steps.rs"]
mod search_steps;
#[path = "e2e/transactions_steps.rs"]
mod transactions_steps;
#[path = "e2e/world.rs"]
mod world;

#[tokio::main]
async fn main() {
    world::LedgerWorld::run("tests/e2e/features").await;
}
