use cucumber::World;

#[path = "e2e/world.rs"]
mod world;
#[path = "e2e/common.rs"]
mod common;
#[path = "e2e/transactions_steps.rs"]
mod transactions_steps;
#[path = "e2e/accounts_steps.rs"]
mod accounts_steps;

#[tokio::main]
async fn main() {
    world::LedgerWorld::run("tests/e2e/features").await;
}
