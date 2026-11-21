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

mod account_update;
mod balance;
mod batch_import;
mod boot_gate;
mod common;
mod contract;
mod documentation;
mod encryption_gate;
mod error_codes;
mod fund_lookup;
mod instrument_create;
mod instrument_create_fund;
mod instrument_create_stock;
mod instrument_search;
mod investment_migration;
mod investment_split_acceptance;
mod merchant_import;
mod reference_data;
mod signal_delivery;
mod stock_lookup;
mod transaction_crud;
mod transaction_list;
