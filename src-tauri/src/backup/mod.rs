//! 备份域（Backup，#406 域目录化归位，ADR-0056）。
//!
//! 备份/恢复引擎与自动备份调度在本域收口：
//! - [`engine`]：备份引擎——zip 打包（`VACUUM INTO` 一致性快照 + 元数据）、
//!   恢复与恢复前安全备份、schema 版本校验（旧→新迁移、新→旧拒绝）、
//!   受管备份列表与滚动清理（ADR-0007 / ADR-0016）；
//! - [`auto`]：自动备份调度——到期判定纯函数与本地日界门、三种触发入口
//!   （周期到期 / 退出兜底 / 首次兜底）、轮询线程、偏好镜像与退出兜底钩子
//!   （ADR-0016 / ADR-0032）。
//!
//! 依赖方向：本域消费基础设施（`db` / `settings` / `events` / `error` / `fs_util`）
//! 与定时计划域的追补入口及落账置脏注册点（#1090 接缝反转的实现注册侧），
//! 不依赖壳层。壳层 `commands::backup` 只做参数解包、事务壳与信号发射；连接层
//! 写入口提交点与定时追补落账的置脏由本域实现（`after_commit_hook` /
//! `occurrence_dirty_hook`）、壳层启动时接线（ADR-0032 / #1090），`lib` 挂调度
//! 线程与退出兑底。
//!
//! 置脏原语 `mark_dirty` 为域内私有（#1090）：暴露面只有接线用钩子
//! （`after_commit_hook` / `occurrence_dirty_hook`），再公开即红：
//!
//! ```compile_fail
//! use tauri_app_lib::backup::mark_dirty;
//! ```

mod auto;
mod engine;

pub use auto::{
    AUTO_BACKUP_PREFIX, AttemptOutcome, AutoBackupState, BackupDecision, PrefsState, SkipReason,
    after_commit_hook, auto_backup_file_name, due_decision, exit_fallback, get_state,
    install_after_commit_hook, install_occurrence_dirty_hook, occurrence_dirty_hook, reset,
    run_due_backup, run_exit_backup, run_first_backup, seed_book_scope, set_state, shared_prefs,
    start_scheduler,
};
pub use engine::{
    BackupFileInfo, BackupKind, BackupMetaSummary, BackupResult, BackupScope, PruneResult,
    RestoreResult, backup_db_to, expected_schema_version, list_managed_backups, probe_backup_meta,
    prune_managed_backups, read_backup_kind, read_backup_meta, restore_db_from,
};

/// 备份目录镜像推送（壳层 `set_auto_backup_dir`）等待数据库连接锁的原语：
/// 调度线程 / 退出兑底 / 首次兑底共享同一超时语义。
pub(crate) use auto::lock_conn_with_timeout;

#[cfg(test)]
mod tests;
