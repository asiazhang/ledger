#![allow(clippy::unreachable)]

use std::sync::RwLock;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::data_location::{default_data_dir, effective_db_dir_of};
use crate::commands::encryption::resume_business_surface;
use ledger_infra::db::boot::{
    BOOT_DB_NORMALIZE_FAILED, BOOT_DB_UNREADABLE, BootFailureGate, BootPlan,
};
use ledger_infra::db::data_location::Boot;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::{
    DbState, install_facade, open_connection_in, open_connection_readonly_in, reset_db_file, run_db,
};
use ledger_infra::error::{AppError, Result};

pub type BootCell = RwLock<Boot>;

fn register_boot<R: Runtime>(app: &AppHandle<R>, boot: Boot) {
    match app.try_state::<BootCell>() {
        Some(cell) => {
            let mut guard = cell.write().unwrap_or_else(|e| e.into_inner());
            *guard = boot;
        }
        None => {
            app.manage(BootCell::new(boot));
        }
    }
}

pub(crate) fn current_boot<R: Runtime>(app: &AppHandle<R>) -> Option<Boot> {
    app.try_state::<BootCell>()
        .map(|cell| cell.read().unwrap_or_else(|e| e.into_inner()).clone())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootPhase {
    Ready,
    AwaitUnlock,
    Failed,
}

impl BootPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            BootPhase::Ready => "ready",
            BootPhase::AwaitUnlock => "locked",
            BootPhase::Failed => "failed",
        }
    }
}

fn swap_or_manage_db_state<R: Runtime>(
    app: &AppHandle<R>,
    conn: Connection,
    read_conn: Connection,
) -> Result<()> {
    match app.try_state::<DbState>() {
        Some(existing) => existing.swap_pair(conn, read_conn)?,
        None => {
            let state = DbState::from_pair(conn, read_conn);
            install_facade(&state.slots())?;
            app.manage(state);
        }
    }
    Ok(())
}

fn placeholder_db<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let conn = ledger_infra::db::open_in_memory()?;
    let read_conn = ledger_infra::db::open_in_memory()?;
    swap_or_manage_db_state(app, conn, read_conn)
}

pub(crate) fn detach_read_conn<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<DbState>()
        && let Err(e) = state.placeholderize_read_conn()
    {
        tracing::error!(error = %e, "读连接换出失败，重启前读命令将持续报错");
    }
}

pub(crate) fn boot_sequence<R: Runtime>(app: &AppHandle<R>) -> Result<BootPhase> {
    let default_dir = default_data_dir(app)?;
    std::fs::create_dir_all(&default_dir).map_err(|e| AppError::Io(e.to_string()))?;
    let BootPlan { boot, disposition } = ledger_infra::db::boot::plan_boot(&default_dir);
    if let Some(reason) = &boot.fallback_reason {
        tracing::warn!(reason = %reason, "DataLocation 引导发生回退，已改用默认数据目录");
    }
    let db_dir = boot.db_dir.clone();
    register_boot(app, boot);
    let gate = app.state::<EncryptionGate>();
    let boot_gate = app.state::<BootFailureGate>();
    let disposition = disposition?;
    match disposition {
        ledger_infra::db::boot::BootDisposition::AwaitUnlock => {
            gate.set_locked(true);
            placeholder_db(app)?;
            tracing::info!(db_dir = %db_dir.display(), "检测到密文库，等待解锁");
            Ok(BootPhase::AwaitUnlock)
        }
        ledger_infra::db::boot::BootDisposition::OpenPlaintext
        | ledger_infra::db::boot::BootDisposition::NormalizePlaintext => {
            if disposition == ledger_infra::db::boot::BootDisposition::NormalizePlaintext {
                normalize_plaintext_db_file(&db_dir)?;
            }
            let conn = open_connection_in(&db_dir)?;
            let read_conn = open_connection_readonly_in(&db_dir)?;
            swap_or_manage_db_state(app, conn, read_conn)?;
            gate.set_locked(false);
            boot_gate.clear();
            crate::shell_support::logger::apply_persisted_level_via_state(app)?;
            tracing::info!(db_dir = %db_dir.display(), "数据库初始化完成");
            Ok(BootPhase::Ready)
        }
        ledger_infra::db::boot::BootDisposition::Unreadable => Err(AppError::coded(
            BOOT_DB_UNREADABLE,
            "数据库文件无法打开（文件可能已损坏）",
        )),
    }
}

fn normalize_plaintext_db_file(db_dir: &std::path::Path) -> Result<()> {
    let db_path = db_dir.join(ledger_infra::db::data_location::DB_FILE_NAME);
    ledger_infra::db::encryption::normalize_plaintext_db_file(&db_path).map_err(|e| {
        tracing::error!(error = %e, "外来形态明文库归一化失败，原库保持原样");
        AppError::coded(
            BOOT_DB_NORMALIZE_FAILED,
            "账本库文件形态异常，无法自动修复；请重启应用重试，或从备份文件恢复",
        )
    })
}

pub(crate) fn recover_boot_failure<R: Runtime>(app: &AppHandle<R>, error: &AppError) -> Result<()> {
    tracing::error!(error = %error, "数据库初始化失败，登记启动失败状态，交由前端失败恢复屏接管");
    app.state::<BootFailureGate>().set_failed(error.code());
    placeholder_db(app)
}

pub(crate) fn try_boot_sequence<R: Runtime>(app: &AppHandle<R>) -> BootPhase {
    match boot_sequence(app) {
        Ok(phase) => phase,
        Err(e) => {
            if let Err(recover_err) = recover_boot_failure(app, &e) {
                tracing::error!(error = %recover_err, "占位内存库登记失败，保持启动失败态");
            }
            BootPhase::Failed
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BootStatus {
    pub phase: &'static str,
    pub error_code: Option<String>,
}

#[tauri::command]
pub fn get_boot_status<R: Runtime>(app: AppHandle<R>) -> Result<BootStatus> {
    let failed = app.state::<BootFailureGate>().is_failed();
    let locked = app.state::<EncryptionGate>().is_locked();
    let phase = if failed {
        BootPhase::Failed
    } else if locked {
        BootPhase::AwaitUnlock
    } else {
        BootPhase::Ready
    };
    let status = BootStatus {
        phase: phase.as_str(),
        error_code: (phase == BootPhase::Failed)
            .then(|| app.state::<BootFailureGate>().failure_code()),
    };
    tracing::info!(phase = status.phase, "前端启动探测完成（WebView 已加载）");
    Ok(status)
}

#[tauri::command]
pub async fn restart_app<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    tracing::info!("应用重启开始：原位重引导（进程不退出），完成后由前端重载 WebView");
    let handle = app.clone();
    ledger_sync_engine::SessionEnvelope::forget();
    let phase = run_db("restart_app", move || Ok(try_boot_sequence(&handle))).await?;
    if phase == BootPhase::Ready {
        crate::start_background_services(&app);
    }
    tracing::info!(
        phase = phase.as_str(),
        "原位重引导完成，等待前端重载 WebView"
    );
    Ok(())
}

#[tauri::command]
pub async fn reset_after_startup_failure<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    let gate = app.state::<BootFailureGate>();
    if !gate.is_failed() {
        return Err(AppError::coded(
            "boot.not-failed",
            "应用未处于启动失败状态，无需重置",
        ));
    }
    let db_dir = effective_db_dir_of(&app)?;
    let (conn, read_conn) = run_db("reset_after_startup_failure", move || {
        let conn = reset_db_file(&db_dir)?;
        let read_conn = open_connection_readonly_in(&db_dir)?;
        Ok((conn, read_conn))
    })
    .await?;
    resume_business_surface(
        &app,
        ledger_sync_engine::SessionEnvelope::Plaintext,
        conn,
        read_conn,
    )?;
    app.state::<BootFailureGate>().clear();
    tracing::info!("启动失败重置完成：旧库保留 .bak 副本，应用以全新明文空库进入");
    Ok(())
}
