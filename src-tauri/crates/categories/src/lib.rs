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
mod core;
mod model;

pub use command::replay_command;
pub use command::{CategoryCommand, CategoryCommandRow};
pub use core::{
    create_category, create_category_idempotent, delete_category, list_categories,
    reorder_categories, update_category,
};
pub use model::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};

#[cfg(test)]
mod tests;
