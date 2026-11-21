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

mod accounts;
mod app_info;
mod app_update;
mod books;
mod boot;
mod budget;
mod instrument_sync;
mod savings_goal;
mod sync_channel;
mod sync_checkpoint;
mod sync_trigger;
