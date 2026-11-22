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

mod command;
mod crud;
mod model;
mod pace;
mod progress;
pub use command::{SavingsGoalCommand, replay_command};
pub use crud::{
    archive_savings_goal, create_savings_goal, delete_savings_goal, ensure_account_can_archive,
    ensure_account_not_goal_bound, unarchive_savings_goal, update_savings_goal,
};
pub use model::{
    SavingsGoal, SavingsGoalInput, SavingsGoalPaceSource, SavingsGoalProgress, SavingsGoalStatus,
    SavingsGoalUpdateInput,
};
pub use pace::{SavingsGoalProjection, linked_plan_pace_monthly};
pub use progress::list_savings_goal_progress;

#[cfg(test)]
mod tests;
