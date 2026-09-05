//! 备份/恢复 IPC 命令壳（issue #91；#406 域目录化后压平为单文件纯壳）。
//!
//! 只做参数解包、事务壳（连接锁边界）与信号发射；备份引擎（zip 打包 / 恢复与
//! 安全备份 / schema 校验 / 受管备份清理）与自动备份调度（到期判定、日界门、
//! 触发入口）在 `crate::backup` 域目录。对外暴露 `create_backup` / `restore_backup`
//! / `restart_app` / `list_backups` / `prune_backups` / `set_auto_backup_dir` /
//! `get_auto_backup_state` / `set_auto_backup_enabled` 命令（`commands/mod.rs` 经
//! `pub use backup::*` 重导出，注册路径与前端/BDD 调用零改动）。
//!
//! 触碰 DB 与阻塞文件 IO 的命令 async 化（形状乙，spec #498 / #503）：DB/zip/
//! 目录扫描等阻塞工作经连接层统一 helper [`crate::db::run_db`] 进 tauri 阻塞
//! 线程池执行，不占用界面事件循环线程；信号在 await 后发射。`restart_app`
//! 是系统控制命令（进程重启，无阻塞工作面），保持同步形态（先例：
//! `set_auto_execution_enabled`）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::backup;
use crate::backup::{
    BackupFileInfo, BackupKind, BackupMetaSummary, BackupResult, PruneResult, RestoreResult,
    backup_db_to, expected_schema_version, list_managed_backups, probe_backup_meta,
    prune_managed_backups, restore_db_from,
};
use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 把当前数据库备份为 zip 包写入 `target_path`（完整文件路径，含文件名）。
#[tauri::command]
pub async fn create_backup(app: AppHandle, target_path: String) -> Result<BackupResult> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("create_backup", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let app_version = app.package_info().version.to_string();
        backup_db_to(
            &conn,
            Path::new(&target_path),
            &app_version,
            BackupKind::Manual,
        )
    })
    .await
}

/// 从 `backup_path`（zip 或裸 db）恢复数据库。
///
/// 恢复期间持有全局连接锁，阻塞 IPC 与本地 HTTP API 的并发写，避免恢复过程中被写入污染。
/// 恢复成功后由前端调用 `restart_app` 重启应用。
/// `passphrase` 用于密文备份（issue #572 / ADR-0075 决策 7）：备份库为密文时凭
/// 该主口令校验与恢复；明文备份不消费。主口令不落日志与 trace（ADR-0075）。
#[tauri::command]
pub async fn restore_backup(
    app: AppHandle,
    backup_path: String,
    passphrase: Option<String>,
) -> Result<RestoreResult> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("restore_backup", move || {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Io(e.to_string()))?;
        let db_path = dir.join("ledger.db");
        let expected = expected_schema_version()?;
        // 恢复期间持有主连接锁，阻塞 IPC 与本地 HTTP API 的并发写。
        let _guard = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        restore_db_from(
            Path::new(&backup_path),
            &db_path,
            &dir,
            expected,
            passphrase.as_deref(),
        )
    })
    .await
}

/// 读取单个备份文件的元数据摘要（来源 + 加密标记，issue #572）：恢复确认弹窗
/// 据加密标记与当前模式比对显警告、密文备份引导输入主口令。
#[tauri::command]
pub async fn get_backup_meta(path: String) -> Result<BackupMetaSummary> {
    run_db("get_backup_meta", move || {
        probe_backup_meta(Path::new(&path))
    })
    .await
}

/// 重启应用（恢复成功后调用，使新数据以全新状态加载）。
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

/// 列出备份目录中的受管备份文件（自动命名，含手动 `ledger-backup-*` 与
/// 自动 `ledger-auto-*` 两类前缀），按新→旧排序。
#[tauri::command]
pub async fn list_backups(dir: String) -> Result<Vec<BackupFileInfo>> {
    run_db("list_backups", move || {
        list_managed_backups(Path::new(&dir))
    })
    .await
}

/// 将备份目录中的受管备份修剪到最多 `keep` 个（删除最旧的超出部分）。
/// 清理成功后发出 `ledger:backups-changed` 信号（issue #129，经信号映射单点
/// `signals::emit_for` 判定发射，ADR-0044），前端列表随之自动刷新。
#[tauri::command]
pub async fn prune_backups(app: AppHandle, dir: String, keep: i64) -> Result<PruneResult> {
    let r = run_db("prune_backups", move || {
        let keep = usize::try_from(keep).map_err(|_| {
            AppError::codedp(
                "backup.keep-invalid",
                format!("备份保留上限非法: {keep}"),
                &[&keep.to_string()],
            )
        })?;
        prune_managed_backups(Path::new(&dir), keep)
    })
    .await?;
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
pub async fn set_auto_backup_dir(app: AppHandle, dir: String) -> Result<()> {
    run_db("set_auto_backup_dir", move || {
        let trimmed = dir.trim();
        let normalized = (!trimmed.is_empty()).then(|| trimmed.to_string());
        let prefs = backup::shared_prefs();
        prefs.set_dir(normalized.clone());
        if normalized.is_none() || !prefs.claim_first_fallback() {
            return Ok(());
        }
        let conn = app.state::<DbState>().conn.clone();
        // 与调度线程/退出兑底一致：拿锁带 5s 超时，拿不到则放弃本轮兑底机会。
        let Some(conn) = backup::lock_conn_with_timeout(&conn) else {
            tracing::warn!("首次兑底等待数据库锁超时，放弃本轮兑底");
            return Ok(());
        };
        let version = app.package_info().version.to_string();
        let _ =
            backup::run_first_backup(&conn, normalized.as_deref(), &version, chrono::Utc::now());
        Ok(())
    })
    .await
}

/// 自动备份设置页状态（issue #128）：开关与上次自动备份时间。
/// 设置页仅需这两项；脏标记为后端调度内部状态，不上 IPC 面。
#[derive(Debug, Serialize)]
pub struct AutoBackupSettingsState {
    pub enabled: bool,
    pub last_backup_at: Option<String>,
}

/// 读取自动备份调度状态（issue #128，设置页展示）：key 缺失或恢复了旧版本备份
/// （`app_settings` 表缺失）时由 [`backup::get_state`] 落到约定默认值。
/// 备份目录是前端 localStorage 偏好（ADR-0016），目录未配置提示由设置页自判。
#[tauri::command]
pub async fn get_auto_backup_state(app: AppHandle) -> Result<AutoBackupSettingsState> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("get_auto_backup_state", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let s = backup::get_state(&conn)?;
        Ok(AutoBackupSettingsState {
            enabled: s.enabled,
            last_backup_at: s.last_backup_at,
        })
    })
    .await
}

/// 设置自动备份开关（issue #128）：写入 `ledger.db` 的 `app_settings`
/// （经 [`crate::settings`] 收口），调度线程下次检查即刻生效；目录镜像不动。
#[tauri::command]
pub async fn set_auto_backup_enabled(app: AppHandle, enabled: bool) -> Result<()> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("set_auto_backup_enabled", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        crate::settings::set(
            &conn,
            crate::settings::SettingKey::AutoBackupEnabled,
            &enabled,
        )
    })
    .await
}
