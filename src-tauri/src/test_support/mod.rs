#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable
)]

mod assert;
pub mod channel;
mod home_isolation;
pub mod op_write_failure;
pub mod s3;
pub mod scan;
pub mod scratch;
mod seed;
pub mod snapshot_probe;
#[cfg(test)]
mod tests;
pub mod zip_meta;

pub use assert::{
    assert_balance_cache_matches_realtime, extract_check_in_literals, read_scalar_i64,
};
pub use channel::{publish_raw_legacy_checkpoint, publish_raw_segment};
pub use home_isolation::isolate_home;
pub use op_write_failure::{assert_op_write_failure, block_op_writes, unblock_op_writes};
pub use s3::{
    S3Addressing, S3Deny, S3Gate, S3ObservedRequest, S3Stub, S3StubConfig, spawn_s3_stub,
};
pub use scratch::{ScratchDir, ScratchFile};
pub use seed::{
    seed_account, seed_exchange_rate, seed_exchange_rate_with_source, seed_fund_market_price,
    seed_fx_history_weeks, seed_fx_rate_history, seed_instrument, seed_investment_setup,
    seed_market_price, seed_price_history,
};
pub use zip_meta::merge_extra_backup_meta;

use std::future::Future;
use std::path::Path;

use rusqlite::Connection;

pub const FIXED_NOW: &str = "2026-01-01T00:00:00Z";

pub fn block_on<F: Future>(future: F) -> F::Output {
    tauri::async_runtime::block_on(future)
}

pub fn open() -> Connection {
    install_test_wiring();
    ledger_infra::db::open_in_memory_initialized().expect("打开并初始化内存测试库")
}

pub fn open_file(db_dir: &Path) -> Connection {
    install_test_wiring();
    ledger_infra::db::open_connection_in(db_dir).expect("打开并初始化文件测试库")
}

pub fn open_file_scratch(tag: &str) -> (Connection, ScratchDir) {
    let dir = ScratchDir::new(tag);
    let conn = open_file(dir.path());
    (conn, dir)
}

fn install_test_wiring() {
    ledger_backup::install_after_commit_hook();
    ledger_sync_engine::trigger::install_after_write_hook();
    ledger_accounts::balance::install_balance_refresh_hook();
    ledger_scheduled::install_plan_source_hook();
    ledger_scheduled::auto_run::register_after_occurrence_hook(
        ledger_backup::occurrence_dirty_hook,
    );
    ledger_backup::register_catch_up_hook(ledger_scheduled::auto_run::catch_up_hook);
    crate::transaction_wiring::install_all();
}
