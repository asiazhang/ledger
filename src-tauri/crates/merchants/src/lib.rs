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

pub use command::MerchantCommand;
pub use command::replay_command;
pub use crud::{
    create_merchant, create_merchant_by_name, delete_merchant, find_merchant_by_name, get_merchant,
    install_merchant_hooks, list_merchants, transaction_counts, update_merchant,
};
pub use model::{Merchant, MerchantInput, MerchantTransactionCount, MerchantUpdateInput};

#[cfg(test)]
mod tests;
