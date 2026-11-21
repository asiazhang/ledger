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
mod engine;
mod models;
mod source;
mod spend;

pub mod auto_run;

pub use auto_run::*;
pub use command::replay_command;
pub use command::{ScheduledCommand, occurrence_transaction_id};
pub use engine::*;
pub use models::{
    CreateScheduledInput, ExecuteOccurrenceInput, InstallmentPlan, OccurrenceStatus,
    RecurrenceType, ScheduledKind, ScheduledStatus, ScheduledTransaction,
    ScheduledTransactionDetail, ScheduledTransactionOccurrence, ScheduledTransactionWithExt,
    ScheduledTransferPlan, SubscriptionPlan, UpdateStatusInput, UpdateSubscriptionInput,
};
pub use source::install_plan_source_hook;
pub use spend::*;

#[cfg(test)]
mod tests;
