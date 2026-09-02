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
//! 与定时计划域的追补入口，不依赖壳层。壳层 `commands::backup` 只做参数解包、
//! 事务壳与信号发射；连接层写入口提交点经 [`mark_dirty`] / [`run_due_backup`]
//! 组合置脏与写时顺带检查（ADR-0032），`lib` 挂调度线程与退出兜底。

mod auto;
mod engine;

pub use auto::{
    AUTO_BACKUP_PREFIX, AttemptOutcome, AutoBackupState, BackupDecision, PrefsState, SkipReason,
    auto_backup_file_name, due_decision, exit_fallback, get_state, reset, run_due_backup,
    run_exit_backup, run_first_backup, set_state, shared_prefs, start_scheduler,
};
pub use engine::{
    BackupFileInfo, BackupKind, BackupResult, PruneResult, RestoreResult, backup_db_to,
    expected_schema_version, list_managed_backups, prune_managed_backups, read_backup_kind,
    restore_db_from,
};

/// 连接层写入口提交点（`db::write` → after_commit，ADR-0032）与定时追补落账的
/// 置脏原语：非业务公开 API，仅进程内结构性调用点可见。
pub(crate) use auto::mark_dirty;

/// 备份目录镜像推送（壳层 `set_auto_backup_dir`）等待数据库连接锁的原语：
/// 调度线程 / 退出兜底 / 首次兜底共享同一超时语义。
pub(crate) use auto::lock_conn_with_timeout;

#[cfg(test)]
mod tests;
