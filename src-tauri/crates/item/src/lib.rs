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

pub mod command;
pub mod cost;
pub mod domain;
pub mod guard;
mod model;

pub use command::{ItemCommand, ItemCommandRow};
pub use domain::*;
pub use model::{
    Item, ItemDailyCost, ItemDisposeInput, ItemInput, ItemSourceDisplay, ItemStatus,
    ItemWithDailyCost,
};

pub use command::replay_command;

#[cfg(test)]
mod tests;
