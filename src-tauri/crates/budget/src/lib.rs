#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

mod annual_summary;
mod command;
pub mod crud;
mod model;
mod progress;

pub use annual_summary::{annual_budget_total_cents, budget_annual_summary};
pub use command::BudgetCommand;
pub use command::replay_command;
pub use crud::{create_budget, delete_budget, list_budgets, update_budget};
pub use model::{
    Budget, BudgetAnnualSummary, BudgetInput, BudgetPeriod, BudgetProgress, BudgetUpdateInput,
};
pub use progress::budget_progress_rows;

#[cfg(test)]
mod tests;
