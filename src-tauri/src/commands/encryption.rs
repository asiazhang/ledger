#![allow(clippy::unreachable)]

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::boot::current_boot;
use crate::commands::data_location::default_data_dir;
use ledger_infra::db::DbState;
use ledger_infra::db::data_location::{self, DB_FILE_NAME};
use ledger_infra::db::encryption::{self, DbFileKind, EncryptionGate};
use ledger_infra::db::passphrase_cache::{self, CacheLoad, RememberMode};
use ledger_infra::db::run_db;
use ledger_infra::error::{AppError, Result};

#[derive(Debug, Serialize)]
pub struct EncryptionStatus {
    pub locked: bool,
    pub file_encrypted: bool,
}

#[derive(Debug, Serialize)]
pub struct UnlockOutcome {
    pub relocated: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RememberPassphraseSupport {
    pub supported: bool,
    pub mode: RememberMode,
}

pub(crate) fn active_db_path<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf> {
    let db_dir = match current_boot(app) {
        Some(boot) => boot.db_dir,
        None => default_data_dir(app)?,
    };
    Ok(db_dir.join(DB_FILE_NAME))
}

pub(crate) fn active_book_id<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    current_boot(app)
        .and_then(|boot| boot.registry)
        .map(|registry| registry.active_id)
}

fn ensure_unlocked<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    if app.state::<EncryptionGate>().is_locked() {
        return Err(AppError::coded(
            "encryption.locked",
            "应用已锁定，请先解锁后再操作",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn get_encryption_status(app: AppHandle) -> Result<EncryptionStatus> {
    let locked = app.state::<EncryptionGate>().is_locked();
    let db_path = active_db_path(&app)?;
    run_db("get_encryption_status", move || {
        let file_encrypted = encryption::probe_file_kind(&db_path)? == DbFileKind::Encrypted;
        Ok(EncryptionStatus {
            locked,
            file_encrypted,
        })
    })
    .await
}

async fn do_unlock<R: Runtime>(app: &AppHandle<R>, passphrase: &str) -> Result<UnlockOutcome> {
    let db_path = active_db_path(app)?;
    let pass = passphrase.to_string();
    let (conn, read_conn) = run_db("unlock_encryption_core", move || {
        let conn = encryption::unlock_db_file(&db_path, &pass)?;
        let read_conn =
            ledger_infra::db::open_connection_readonly_with_passphrase(&db_path, &pass)?;
        Ok((conn, read_conn))
    })
    .await?;
    resume_business_surface(
        app,
        ledger_sync_engine::SessionEnvelope::Encrypted(passphrase.to_string()),
        conn,
        read_conn,
    )?;

    let mut relocated = false;
    let pending = current_boot(app).and_then(|boot| boot.deferred_relocation);
    if let Some(target_dir) = pending {
        let source = active_db_path(app)?;
        let target = target_dir.join(DB_FILE_NAME);
        let pass = passphrase.to_string();
        let outcome = run_db("unlock_deferred_relocation", move || {
            data_location::relocate_with_key(&source, &target, &pass)
        })
        .await;
        match outcome {
            Ok(()) => {
                relocated = true;
                tracing::info!(target = %target_dir.display(), "解锁后搬迁完成，待重启接管目标位置");
            }
            Err(e) => {
                tracing::warn!(error = %e, "解锁后搬迁失败，保持当前位置运行");
            }
        }
    }
    Ok(UnlockOutcome { relocated })
}

#[tauri::command]
pub async fn unlock_encryption<R: Runtime>(
    app: AppHandle<R>,
    passphrase: String,
) -> Result<UnlockOutcome> {
    let gate = app.state::<EncryptionGate>();
    if !gate.is_locked() {
        return Err(AppError::coded(
            "encryption.not-locked",
            "应用未处于锁定状态，无需解锁",
        ));
    }
    do_unlock(&app, &passphrase).await
}

#[tauri::command]
pub async fn unlock_with_remembered_passphrase<R: Runtime>(
    app: AppHandle<R>,
) -> Result<UnlockOutcome> {
    let gate = app.state::<EncryptionGate>();
    if !gate.is_locked() {
        return Err(AppError::coded(
            "encryption.not-locked",
            "应用未处于锁定状态，无需解锁",
        ));
    }
    tracing::info!("自动解锁开始：读取钥匙串缓存");
    let book = active_book_id(&app);
    let started = std::time::Instant::now();
    let cached = run_db("read_remembered_passphrase", move || {
        passphrase_cache::load(book.as_deref())
    })
    .await;
    let outcome = match &cached {
        Ok(CacheLoad::Found(_)) => "found",
        Ok(CacheLoad::NotFound) => "not-found",
        Ok(CacheLoad::Cancelled) => "cancelled",
        Err(_) => "error",
    };
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        outcome,
        "钥匙串缓存读取完成"
    );
    let cached = cached?;
    match cached {
        CacheLoad::Found(passphrase) => match do_unlock(&app, &passphrase).await {
            Ok(outcome) => Ok(outcome),
            Err(e) if e.is_code("encryption.passphrase-incorrect") => {
                tracing::warn!("本机缓存的主口令已过期，清理缓存以回退手输");
                let book = active_book_id(&app);
                let _ = run_db("clear_stale_remembered_passphrase", move || {
                    passphrase_cache::delete(book.as_deref())
                })
                .await;
                Err(e)
            }
            Err(e) => Err(e),
        },
        CacheLoad::NotFound => Err(AppError::coded(
            "encryption.remember-no-cache",
            "本机没有缓存的主口令，请手动输入",
        )),
        CacheLoad::Cancelled => Err(AppError::coded(
            "encryption.remember-biometric-cancelled",
            "生物认证已取消，请手动输入主口令",
        )),
    }
}

pub(crate) fn resume_business_surface<R: Runtime>(
    app: &AppHandle<R>,
    session: ledger_sync_engine::SessionEnvelope,
    conn: Connection,
    read_conn: Connection,
) -> Result<()> {
    ledger_sync_engine::SessionEnvelope::remember(session);
    app.state::<DbState>().swap_pair(conn, read_conn)?;
    app.state::<EncryptionGate>().set_locked(false);
    crate::shell_support::logger::apply_persisted_level_via_state(app)?;
    tracing::info!("业务读写恢复（解锁/重置后），自动备份与多端同步调度拉起");
    crate::start_background_services(app);
    Ok(())
}

#[tauri::command]
pub async fn enable_encryption<R: Runtime>(app: AppHandle<R>, passphrase: String) -> Result<()> {
    ensure_unlocked(&app)?;
    let db_path = active_db_path(&app)?;
    run_db("enable_encryption", move || {
        encryption::enable_encryption_for_file(&db_path, &passphrase)
    })
    .await?;
    crate::commands::boot::detach_read_conn(&app);
    tracing::info!("整库加密转换完成，待重启以新口令重新打开");
    Ok(())
}

#[tauri::command]
pub async fn disable_encryption<R: Runtime>(app: AppHandle<R>, passphrase: String) -> Result<()> {
    ensure_unlocked(&app)?;
    let db_path = active_db_path(&app)?;
    run_db("disable_encryption", move || {
        encryption::disable_encryption_for_file(&db_path, &passphrase)
    })
    .await?;
    let book = active_book_id(&app);
    let _ = run_db("clear_remember_after_disable", move || {
        passphrase_cache::delete(book.as_deref())
    })
    .await;
    ledger_sync_engine::SessionEnvelope::remember(ledger_sync_engine::SessionEnvelope::Plaintext);
    crate::commands::boot::detach_read_conn(&app);
    tracing::info!("整库转换完成（关闭加密），待重启以明文重新打开");
    Ok(())
}

#[tauri::command]
pub async fn change_encryption_passphrase<R: Runtime>(
    app: AppHandle<R>,
    passphrase: String,
    new_passphrase: String,
) -> Result<()> {
    ensure_unlocked(&app)?;
    let db_path = active_db_path(&app)?;
    run_db("change_encryption_passphrase", move || {
        encryption::change_passphrase_for_file(&db_path, &passphrase, &new_passphrase)
    })
    .await?;
    crate::commands::boot::detach_read_conn(&app);
    tracing::info!("整库转换完成（修改主口令），待重启以新口令重新打开");
    Ok(())
}

#[tauri::command]
pub async fn reset_after_forgotten_passphrase<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    let gate = app.state::<EncryptionGate>();
    if !gate.is_locked() {
        return Err(AppError::coded(
            "encryption.not-locked",
            "应用未处于锁定状态，无需重置",
        ));
    }
    let db_path = active_db_path(&app)?;
    let (conn, read_conn) = run_db("reset_after_forgotten_passphrase", move || {
        let conn = encryption::reset_encrypted_db_file(&db_path)?;
        let read_conn = ledger_infra::db::open_connection_readonly(&db_path)?;
        Ok((conn, read_conn))
    })
    .await?;
    let book = active_book_id(&app);
    let _ = run_db("clear_remember_after_reset", move || {
        passphrase_cache::delete(book.as_deref())
    })
    .await;
    resume_business_surface(
        &app,
        ledger_sync_engine::SessionEnvelope::Plaintext,
        conn,
        read_conn,
    )?;
    tracing::info!("忘记口令重置完成，应用以全新明文空库回到明文模式");
    Ok(())
}

#[tauri::command]
pub async fn get_remember_passphrase_support() -> Result<RememberPassphraseSupport> {
    Ok(RememberPassphraseSupport {
        supported: passphrase_cache::supported(),
        mode: passphrase_cache::current_mode(),
    })
}

#[tauri::command]
pub async fn set_remember_passphrase<R: Runtime>(
    app: AppHandle<R>,
    passphrase: String,
) -> Result<()> {
    ensure_unlocked(&app)?;
    let book = active_book_id(&app);
    run_db("set_remember_passphrase", move || {
        passphrase_cache::store(&passphrase, book.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn clear_remember_passphrase<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    ensure_unlocked(&app)?;
    let book = active_book_id(&app);
    run_db("clear_remember_passphrase", move || {
        passphrase_cache::delete(book.as_deref())
    })
    .await
}
