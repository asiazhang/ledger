//! 备份/恢复（issue #91）：命令外壳 + 核心逻辑 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `core`：核心逻辑——zip 打包/恢复/受管备份列表与修剪/schema 版本校验（原 backup.rs 非命令部分，保持原状不拆分）；
//! - `tests`：原内嵌测试外迁。
//!
//! 对外仅暴露 `create_backup` / `restore_backup` / `restart_app` / `list_backups` /
//! `prune_backups` 命令与 `backup_db_to` 等复用函数（`commands/mod.rs` 经
//! `pub use backup::*` 重导出，注册路径与前端/BDD 调用零改动）。

mod core;
#[cfg(test)]
mod tests;

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{
    auto_backup,
    db::DbState,
    error::{AppError, Result},
    signals::{WriteEvidence, WriteOp, emit_for},
};

pub use core::*;

/// 把当前数据库备份为 zip 包写入 `target_path`（完整文件路径，含文件名）。
#[tauri::command]
pub fn create_backup(app: AppHandle, target_path: String) -> Result<BackupResult> {
    let state = app.state::<DbState>();
    let conn = state.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let app_version = app.package_info().version.to_string();
    core::backup_db_to(
        &conn,
        Path::new(&target_path),
        &app_version,
        core::BackupKind::Manual,
    )
}

/// 从 `backup_path`（zip 或裸 db）恢复数据库。
///
/// 恢复期间持有全局连接锁，阻塞 IPC 与本地 HTTP API 的并发写，避免恢复过程中被写入污染。
/// 恢复成功后由前端调用 `restart_app` 重启应用。
#[tauri::command]
pub fn restore_backup(app: AppHandle, backup_path: String) -> Result<RestoreResult> {
    let state = app.state::<DbState>();
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Io(e.to_string()))?;
    let db_path = dir.join("ledger.db");
    let expected = core::expected_schema_version()?;
    // 恢复期间持有主连接锁，阻塞 IPC 与本地 HTTP API 的并发写。
    let _guard = state.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    core::restore_db_from(Path::new(&backup_path), &db_path, &dir, expected)
}

/// 重启应用（恢复成功后调用，使新数据以全新状态加载）。
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

/// 列出备份目录中的受管备份文件（自动命名，含手动 `ledger-backup-*` 与
/// 自动 `ledger-auto-*` 两类前缀），按新→旧排序。
#[tauri::command]
pub fn list_backups(dir: String) -> Result<Vec<BackupFileInfo>> {
    core::list_managed_backups(Path::new(&dir))
}

/// 将备份目录中的受管备份修剪到最多 `keep` 个（删除最旧的超出部分）。
/// 清理成功后发出 `ledger:backups-changed` 信号（issue #129，经信号映射单点
/// `signals::emit_for` 判定发射，ADR-0044），前端列表随之自动刷新。
#[tauri::command]
pub fn prune_backups(app: AppHandle, dir: String, keep: i64) -> Result<PruneResult> {
    let keep = usize::try_from(keep)
        .map_err(|_| AppError::Invalid(format!("备份保留上限非法: {keep}")))?;
    let r = core::prune_managed_backups(Path::new(&dir), keep)?;
    emit_for(&app, WriteOp::PruneBackups, WriteEvidence::None);
    Ok(r)
}

/// 同步设备本地备份目录到后端（自动备份调度用，ADR-0016 决策 3 的偏好镜像：
/// `backupDir` 保持前端 localStorage 单一来源，启动/变更时推送给后端消费）。
/// 空串视为未配置，自动备份一律静默跳过。
///
/// 本会话首次提供有效目录时，若受管备份列表为空且开关开启，立即执行一次
/// 「首次兜底」备份（issue #125；每会话至多一次，结果只记日志不上抛）。
#[tauri::command]
pub fn set_auto_backup_dir(app: AppHandle, dir: String) -> Result<()> {
    let trimmed = dir.trim();
    let normalized = (!trimmed.is_empty()).then(|| trimmed.to_string());
    let prefs = auto_backup::shared_prefs();
    prefs.set_dir(normalized.clone());
    if normalized.is_none() || !prefs.claim_first_fallback() {
        return Ok(());
    }
    let state = app.state::<DbState>();
    // 与调度线程/退出兑底一致：拿锁带 5s 超时，拿不到则放弃本轮兑底机会。
    let Some(conn) = auto_backup::lock_conn_with_timeout(&state.conn) else {
        tracing::warn!("首次兑底等待数据库锁超时，放弃本轮兑底");
        return Ok(());
    };
    let version = app.package_info().version.to_string();
    let _ =
        auto_backup::run_first_backup(&conn, normalized.as_deref(), &version, chrono::Utc::now());
    Ok(())
}

/// 自动备份设置页状态（issue #128）：开关与上次自动备份时间。
/// 设置页仅需这两项；脏标记为后端调度内部状态，不上 IPC 面。
#[derive(Debug, Serialize)]
pub struct AutoBackupSettingsState {
    pub enabled: bool,
    pub last_backup_at: Option<String>,
}

/// 读取自动备份调度状态（issue #128，设置页展示）：key 缺失或恢复了旧版本备份
/// （`app_settings` 表缺失）时由 [`auto_backup::get_state`] 落到约定默认值。
/// 备份目录是前端 localStorage 偏好（ADR-0016），目录未配置提示由设置页自判。
#[tauri::command]
pub fn get_auto_backup_state(app: AppHandle) -> Result<AutoBackupSettingsState> {
    let state = app.state::<DbState>();
    let conn = state.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let s = auto_backup::get_state(&conn)?;
    Ok(AutoBackupSettingsState {
        enabled: s.enabled,
        last_backup_at: s.last_backup_at,
    })
}

/// 设置自动备份开关（issue #128）：写入 `ledger.db` 的 `app_settings`
/// （经 [`crate::settings`] 收口），调度线程下次检查即刻生效；目录镜像不动。
#[tauri::command]
pub fn set_auto_backup_enabled(app: AppHandle, enabled: bool) -> Result<()> {
    let state = app.state::<DbState>();
    let conn = state.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::settings::set(
        &conn,
        crate::settings::SettingKey::AutoBackupEnabled,
        &enabled,
    )
}
