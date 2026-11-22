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
mod insurer;
mod model;
mod stats;
mod validation;

pub use command::{InsurerCommand, PolicyCommand, PolicyCommandRow};
pub use command::{replay_insurer_command, replay_policy_command};

pub use crud::{
    create_policy, delete_policy, install_source_hook, list_policies, source_display_by_ids,
    update_policy,
};
pub use insurer::{
    Insurer, InsurerInput, InsurerUpdateInput, create_insurer, create_insurer_by_name,
    delete_insurer, find_insurer_by_name, list_insurers, update_insurer,
};
pub use model::{Policy, PolicyInput, PolicySourceDisplay, PolicyStats};
pub use stats::policy_stats;

#[cfg(test)]
mod tests;
