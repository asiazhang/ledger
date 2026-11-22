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
//! use ledger_backup::mark_dirty;
//! ```

use std::time::Duration;

pub const LOCK_HOLD_BUDGET: Duration = Duration::from_secs(30);

mod attribution;
mod auto;
mod engine;
mod identity;

pub use auto::{
    AUTO_BACKUP_PREFIX, AttemptOutcome, AutoBackupState, BackupDecision, FAILURE_ALERT_THRESHOLD,
    LOCK_TIMEOUT, PrefsState, SkipReason, after_commit_hook, auto_backup_file_name, due_decision,
    exit_fallback, failure_alerting, get_state, install_after_commit_hook, lock_conn_with_timeout,
    lock_conn_within, occurrence_dirty_hook, register_catch_up_hook, reset, run_due_backup,
    run_exit_backup, run_first_backup, set_state, shared_prefs, start_scheduler,
};
pub use engine::{
    BackupFileInfo, BackupKind, BackupMetaSummary, BackupResult, BackupScope,
    PRE_UPDATE_BACKUP_FILE_NAME, PruneResult, RestoreResult, backup_db_to,
    create_pre_update_backup, expected_schema_version, list_managed_backups,
    managed_command_target, probe_backup_meta, prune_managed_backups, read_backup_kind,
    read_backup_meta, restore_db_from,
};
pub use identity::{book_fingerprint_readonly, source_device_id};

#[cfg(test)]
mod tests;
