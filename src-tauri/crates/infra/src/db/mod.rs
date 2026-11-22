pub mod connection;
pub mod facade;
pub mod facade_handles;
pub(crate) mod job_gate;
pub mod migrate;
pub mod perf_trace;
pub mod query;
pub mod runtime;
pub mod schema_guard;
pub mod tx_scope;

pub use connection::SYNC_SPLIT_USER_VERSION;
pub use connection::{
    CONCURRENT_BUSY_TIMEOUT, SYNC_DB_FILE_NAME, SYNC_MOUNT_FAILED, SYNC_TABLES, check_integrity,
    open_connection, open_connection_in, open_connection_readonly, open_connection_readonly_in,
    open_connection_readonly_with_passphrase, open_connection_unmounted,
    open_connection_with_passphrase, open_connection_with_passphrase_unmounted, open_db_in,
    open_in_memory, open_in_memory_initialized, reset_db_file, reset_db_in,
    snapshot_sync_tables_into, sync_db_path,
};
pub use facade::DbFacade;
#[cfg(test)]
pub(crate) use facade_handles::facade_installed;
pub use facade_handles::{DbReadHandle, DbSlotPair, DbWriteHandle, install_facade};
pub use job_gate::LockOutcome;
pub use migrate::{init_db, migrations, schema_version};
pub use runtime::{
    AfterCommitHook, DbState, LOCK_HOLD_PROBE_THRESHOLD, register_after_commit_hook,
    replace_read_conn_slot, run_db, write_locked,
};
pub use runtime::{probe_lock_hold, probe_lock_hold_within, register_lock_hold_budget};

pub use crate::ids::{DETERMINISTIC_NAMESPACE, deterministic_uuid, iso_at, new_uuid, now_iso};

pub use crate::boot::{
    book_registry, data_location, disposition as boot, encryption, passphrase_cache,
};

#[cfg(test)]
mod tests;
