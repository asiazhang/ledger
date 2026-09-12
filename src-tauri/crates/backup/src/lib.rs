// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。
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

//! 备份域 crate（Backup，spec #1086 / issue #1091 自根包域目录拆出，首个业务
//! 域 crate；域目录化先例 #406，ADR-0056）。
//!
//! 备份/恢复引擎与自动备份调度在本域收口：
//! - [`engine`]：备份引擎——zip 打包（`VACUUM INTO` 一致性快照 + 元数据）、
//!   恢复与恢复前安全备份、schema 版本校验（旧→新迁移、新→旧拒绝）、
//!   受管备份列表与滚动清理（ADR-0007 / ADR-0016）；
//! - [`auto`]：自动备份调度——到期判定纯函数与本地日界门、三种触发入口
//!   （周期到期 / 退出兜底 / 首次兜底）、轮询线程、偏好镜像与退出兜底钩子
//!   （ADR-0016 / ADR-0032）。
//!
//! 依赖方向：本域消费基础设施（`db` / `settings` / `events` / `error` /
//! `fs_util`，`ledger-infra`），对根包（壳层）与任何业务域零依赖——迁移前对
//! 定时计划域的两条引用按注册点反转收敛（挂载点①/④，ADR-0112 决策 5）：
//! 连接层提交点后置动作（置脏 + 写时顺带到期检查，ADR-0032）与调度线程的
//! 追补触发都由本域定义注册点、实现在壳层启动时接线（`after_commit_hook` 由
//! 本域自装；期次落账置脏与追补触发由定时计划域提供实现、壳层对装）。
//!
//! 依赖方向由编译期强制（issue #1091 AC3）：生产依赖面（lib 目标）没有根包
//! `tauri_app_lib`，在本 crate 生产代码里构造对壳层/定时计划域的引用即编译
//! 失败；dev-dependency 环只覆盖测试目标，不构成生产环（根包 Cargo.toml 注释
//! 与本 crate Cargo.toml 注释留痕）。机器面负向核对住结构守门的 crate 依赖
//! 方向（`scripts/check-structure.ts` CRATES + `check-structure.test.ts`
//! 负向夹具）——本 crate 不能用 `use tauri_app_lib::…` 的 compile_fail 文档
//! 用例承载该负向例：dev-dependency 对 doctest 可见，会击穿该形态（先例说明
//! 见 sync-protocol crate 的 Cargo.toml 注释）。
//!
//! 置脏原语 `mark_dirty` 为域内私有（#1090）：暴露面只有接线用钩子
//! （`after_commit_hook` / `occurrence_dirty_hook`），再公开即红：
//!
//! ```compile_fail
//! use ledger_backup::mark_dirty;
//! ```

mod auto;
mod engine;

pub use auto::{
    AUTO_BACKUP_PREFIX, AttemptOutcome, AutoBackupState, BackupDecision, PrefsState, SkipReason,
    after_commit_hook, auto_backup_file_name, due_decision, exit_fallback, get_state,
    install_after_commit_hook, lock_conn_with_timeout, occurrence_dirty_hook,
    register_catch_up_hook, reset, run_due_backup, run_exit_backup, run_first_backup,
    seed_book_scope, set_state, shared_prefs, start_scheduler,
};
pub use engine::{
    BackupFileInfo, BackupKind, BackupMetaSummary, BackupResult, BackupScope, PruneResult,
    RestoreResult, backup_db_to, expected_schema_version, list_managed_backups, probe_backup_meta,
    prune_managed_backups, read_backup_kind, read_backup_meta, restore_db_from,
};

#[cfg(test)]
mod tests;
