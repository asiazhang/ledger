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

pub mod boot;
pub mod closed_set;
pub mod db;
pub mod error;
pub mod events;
pub mod fs_util;
pub mod ids;
pub mod serde_util;
pub mod settings;
pub mod signals;
#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub mod test_utils;
