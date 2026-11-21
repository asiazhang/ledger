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

pub mod balance;

pub use command::replay_command;
pub use command::{AccountCommand, AccountCommandRow};
pub use core::{
    adjust_account_balance, audit_balance_cache, create_account, create_account_idempotent,
    delete_account, ensure_black_hole_account, get_account, install_funding_account_hook,
    list_account_balances_for_api, list_account_balances_with_visibility, list_accounts,
    list_accounts_for_api, set_account_archived, update_account,
};
pub use model::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountType,
    AccountUpdateInput, BalanceCacheAudit, BalanceCacheDrift,
};

#[cfg(test)]
mod tests;
