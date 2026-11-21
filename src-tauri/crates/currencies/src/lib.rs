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

mod base_currency;
mod command;
mod list;
mod model;

pub use base_currency::{
    DEFAULT_BASE_CURRENCY, current_base_currency, install_base_currency_hook, set_base_currency,
};
pub use command::LedgerSettingCommand;
pub use list::list_currencies;
pub use model::{Currency, ExchangeRate, ExchangeRateInput};

pub use command::replay_command;

#[cfg(test)]
mod tests;
