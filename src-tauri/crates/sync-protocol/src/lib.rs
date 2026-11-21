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

//! ```compile_fail
//! use tauri_app_lib::transaction::TransactionCommand;
//! ```

pub mod command;
pub mod device;
pub mod op;
pub mod position;
