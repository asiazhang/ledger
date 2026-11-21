#![allow(clippy::unreachable)]

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::data_location::effective_db_dir_of;
use crate::shell_support::read_entry::read_entry;
use ledger_backup as backup;
use ledger_backup::LOCK_TIMEOUT as BACKUP_LOCK_TIMEOUT;
use ledger_backup::{
    BackupFileInfo, BackupKind, BackupMetaSummary, BackupResult, BackupScope, PruneResult,
    RestoreResult, backup_db_to, book_fingerprint_readonly, expected_schema_version,
    list_managed_backups, probe_backup_meta, prune_managed_backups, restore_db_from,
};
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::{self, DbState, LockOutcome, run_db};
use ledger_infra::error::{AppError, Result};
use ledger_infra::signals::{WriteEvidence, WriteOp, emit_for};

#[tauri::command]
pub async fn create_backup<R: Runtime>(
    app: AppHandle<R>,
    target_path: String,
) -> Result<BackupResult> {
    let db = app.state::<DbState>().write_handle();
    let app_version = app.package_info().version.to_string();
    db.run_raw("create_backup", move |conn| {
        let target = backup::managed_command_target(Path::new(&target_path), conn)?;
        backup_db_to(conn, &target, &app_version, BackupKind::Manual)
    })
    .await
}

#[tauri::command]
pub async fn restore_backup<R: Runtime>(
    app: AppHandle<R>,
    backup_path: String,
    passphrase: Option<String>,
) -> Result<RestoreResult> {
    let conn = app.try_state::<DbState>().map(|state| state.conn.clone());
    let read_conn = app
        .try_state::<DbState>()
        .map(|state| state.read_conn.clone());
    let db_dir = effective_db_dir_of(&app)?;
    let safety_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Io(e.to_string()))?;
    run_db("restore_backup", move || {
        let db_path = db_dir.join(DB_FILE_NAME);
        let expected = expected_schema_version()?;
        let _guard = match &conn {
            Some(conn) => Some(conn.lock().map_err(|e| AppError::Db(e.to_string()))?),
            None => None,
        };
        let result = restore_db_from(
            Path::new(&backup_path),
            &db_path,
            &safety_dir,
            expected,
            passphrase.as_deref(),
        );
        if result.is_ok()
            && let Some(read_conn) = &read_conn
            && let Err(e) = db::replace_read_conn_slot(read_conn, db::open_in_memory()?)
        {
            tracing::error!(error = %e, "恢复后读连接换出失败，重启前读命令将持续报错");
        }
        result
    })
    .await
}

#[tauri::command]
pub async fn get_backup_meta(path: String) -> Result<BackupMetaSummary> {
    run_db("get_backup_meta", move || {
        probe_backup_meta(Path::new(&path))
    })
    .await
}

#[tauri::command]
pub async fn list_backups(app: AppHandle, dir: String) -> Result<Vec<BackupFileInfo>> {
    let conn = app.state::<DbState>().read_handle();
    read_entry(
        "list_backups",
        conn,
        move |conn| match BackupScope::of_conn(conn)? {
            Some(scope) => list_managed_backups(Path::new(&dir), Some(&scope)),
            None => Ok(Vec::new()),
        },
    )
    .await
}

#[tauri::command]
pub async fn get_book_fingerprint(app: AppHandle) -> Result<Option<String>> {
    let conn = app.state::<DbState>().read_handle();
    read_entry("get_book_fingerprint", conn, book_fingerprint_readonly).await
}

#[tauri::command]
pub async fn get_device_id(app: AppHandle) -> Result<Option<String>> {
    let conn = app.state::<DbState>().read_handle();
    read_entry("get_device_id", conn, |conn| {
        Ok(ledger_backup::source_device_id(conn))
    })
    .await
}

#[tauri::command]
pub async fn prune_backups(app: AppHandle, dir: String, keep: i64) -> Result<PruneResult> {
    let conn = app.state::<DbState>().read_handle();
    let r = read_entry("prune_backups", conn, move |conn| {
        let keep = usize::try_from(keep).map_err(|_| {
            AppError::codedp(
                "backup.keep-invalid",
                format!("备份保留上限非法: {keep}"),
                &[&keep.to_string()],
            )
        })?;
        match BackupScope::of_conn(conn)? {
            Some(scope) => prune_managed_backups(Path::new(&dir), keep, Some(&scope)),
            None => Ok(PruneResult {
                kept: 0,
                deleted: Vec::new(),
                failed: Vec::new(),
            }),
        }
    })
    .await?;
    emit_for(&app, WriteOp::PruneBackups, WriteEvidence::None);
    Ok(r)
}

#[tauri::command]
pub async fn set_auto_backup_dir(app: AppHandle, dir: String) -> Result<()> {
    let trimmed = dir.trim();
    let normalized = (!trimmed.is_empty()).then(|| trimmed.to_string());
    let claim = {
        let prefs = backup::shared_prefs();
        prefs.set_dir(normalized.clone());
        prefs.claim_first_fallback()
    };
    if normalized.is_none() || !claim {
        return Ok(());
    }
    run_db("set_auto_backup_dir", move || {
        let conn = app.state::<DbState>().write_handle();
        let version = app.package_info().version.to_string();
        let outcome = conn.run_raw_blocking_within("backup.first-fallback", BACKUP_LOCK_TIMEOUT, {
            move |conn| {
                let _ = backup::run_first_backup(
                    conn,
                    normalized.as_deref(),
                    &version,
                    chrono::Utc::now(),
                );
                Ok(())
            }
        });
        match outcome {
            LockOutcome::Abandoned => {
                tracing::warn!("首次兜底等待数据库连接超时，放弃本轮兜底");
            }
            LockOutcome::Ran(Ok(())) => {}
            LockOutcome::Ran(Err(error)) => {
                tracing::warn!(error = %error, "首次兜底作业失败");
            }
        }
        Ok(())
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct AutoBackupSettingsState {
    pub enabled: bool,
    pub last_backup_at: Option<String>,
    pub consecutive_failures: u32,
    pub failure_alerting: bool,
}

#[tauri::command]
pub async fn get_auto_backup_state(app: AppHandle) -> Result<AutoBackupSettingsState> {
    let conn = app.state::<DbState>().read_handle();
    read_entry("get_auto_backup_state", conn, move |conn| {
        let s = backup::get_state(conn)?;
        Ok(AutoBackupSettingsState {
            enabled: s.enabled,
            last_backup_at: s.last_backup_at,
            consecutive_failures: s.consecutive_failures,
            failure_alerting: backup::failure_alerting(s.consecutive_failures),
        })
    })
    .await
}

#[tauri::command]
pub async fn set_auto_backup_enabled(app: AppHandle, enabled: bool) -> Result<()> {
    let db = app.state::<DbState>().write_handle();
    db.run_raw("set_auto_backup_enabled", move |conn| {
        ledger_infra::settings::set(
            conn,
            ledger_infra::settings::SettingKey::AutoBackupEnabled,
            &enabled,
        )
    })
    .await
}
