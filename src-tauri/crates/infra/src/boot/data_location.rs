use std::path::{Path, PathBuf};

use rusqlite::params;
use serde::Serialize;

use super::book_registry::{
    self, BookRegistry, PendingRelocation, RegistryOrigin, RegistryRead, read_registry,
};
use super::encryption::{DbFileKind, probe_file_kind};
use crate::db::{
    check_integrity, open_connection_unmounted, open_connection_with_passphrase_unmounted,
    sync_db_path,
};
use crate::error::{AppError, Result};
use crate::fs_util::{atomic_write, cleanup, replace_file, temp_sibling};

pub use crate::db::connection::DB_FILE_NAME;

pub const POINTER_FILE_NAME: &str = book_registry::REGISTRY_FILE_NAME;

#[derive(Clone)]
pub struct Boot {
    pub db_dir: PathBuf,
    pub fallback_reason: Option<String>,
    pub deferred_relocation: Option<PathBuf>,
    pub registry: Option<BookRegistry>,
}

pub fn boot(default_dir: &Path) -> Boot {
    match read_registry(default_dir) {
        RegistryRead::Unconfigured => Boot {
            db_dir: default_dir.to_path_buf(),
            fallback_reason: None,
            deferred_relocation: None,
            registry: Some(BookRegistry::single_default(default_dir)),
        },
        RegistryRead::Corrupt(reason) => Boot {
            db_dir: default_dir.to_path_buf(),
            fallback_reason: Some(reason),
            deferred_relocation: None,
            registry: None,
        },
        RegistryRead::Resolved(resolved) => enter_registry(default_dir, resolved),
    }
}

fn enter_registry(default_dir: &Path, mut registry: BookRegistry) -> Boot {
    let Some(book) = registry.active() else {
        tracing::error!("注册表已解析但活动账本指针不可解析，按损坏回退默认目录");
        return Boot {
            db_dir: default_dir.to_path_buf(),
            fallback_reason: Some("账本注册表活动账本指针无法解析，已回退默认位置".into()),
            deferred_relocation: None,
            registry: None,
        };
    };
    let target = book.dir.clone();
    match registry.origin {
        RegistryOrigin::LegacyPointer => relocate_or_adopt(default_dir, &target, registry),
        RegistryOrigin::NewFormat => {
            let pending = registry
                .pending_relocation
                .take_if(|pending| pending.book_id == registry.active_id);
            match pending {
                Some(pending) => enter_pending_relocation(registry, pending, &target),
                None => match ensure_target_dir(&target) {
                    Ok(()) => Boot {
                        db_dir: target,
                        fallback_reason: None,
                        deferred_relocation: None,
                        registry: Some(registry),
                    },
                    Err(reason) => {
                        tracing::warn!(
                            target = %target.display(),
                            reason = %reason,
                            "活动账本目录不可用，回退默认目录"
                        );
                        Boot {
                            db_dir: default_dir.to_path_buf(),
                            fallback_reason: Some(reason),
                            deferred_relocation: None,
                            registry: Some(registry),
                        }
                    }
                },
            }
        }
    }
}

fn enter_pending_relocation(
    mut registry: BookRegistry,
    pending: PendingRelocation,
    target: &Path,
) -> Boot {
    let source = pending.from_dir.clone();
    tracing::info!(
        book = %pending.book_id,
        from = %source.display(),
        to = %target.display(),
        "消费活动账本搬迁意图"
    );
    match enter_target_with_source(&source, target) {
        Ok(()) => {
            registry.pending_relocation = None;
            Boot {
                db_dir: target.to_path_buf(),
                fallback_reason: None,
                deferred_relocation: None,
                registry: Some(registry),
            }
        }
        Err(EnterTargetError::DeferredEncryptedRelocation) => {
            tracing::info!(
                target = %target.display(),
                "来源库为密文库，搬迁待解锁后补做（本次仍以来源位置生效）"
            );
            registry.redirect_active_book(&source);
            registry.pending_relocation = Some(pending);
            Boot {
                db_dir: source,
                fallback_reason: None,
                deferred_relocation: Some(target.to_path_buf()),
                registry: Some(registry),
            }
        }
        Err(EnterTargetError::Failed(reason)) => {
            tracing::warn!(
                target = %target.display(),
                reason = %reason,
                "活动账本搬迁失败，以来源位置生效（意图保留待下次启动重试）"
            );
            registry.pending_relocation = Some(pending);
            Boot {
                db_dir: source,
                fallback_reason: Some(reason),
                deferred_relocation: None,
                registry: Some(registry),
            }
        }
    }
}

fn relocate_or_adopt(default_dir: &Path, target: &Path, mut registry: BookRegistry) -> Boot {
    match enter_target_with_source(default_dir, target) {
        Ok(()) => Boot {
            db_dir: target.to_path_buf(),
            fallback_reason: None,
            deferred_relocation: None,
            registry: Some(registry),
        },
        Err(EnterTargetError::DeferredEncryptedRelocation) => {
            tracing::info!(
                target = %target.display(),
                "源库为密文库，搬迁待解锁后补做（本次仍以源库位置生效）"
            );
            registry.redirect_default_book(default_dir);
            Boot {
                db_dir: default_dir.to_path_buf(),
                fallback_reason: None,
                deferred_relocation: Some(target.to_path_buf()),
                registry: Some(registry),
            }
        }
        Err(EnterTargetError::Failed(reason)) => {
            tracing::warn!(
                target = %target.display(),
                reason = %reason,
                "DataLocation 引导回退默认目录"
            );
            Boot {
                db_dir: default_dir.to_path_buf(),
                fallback_reason: Some(reason),
                deferred_relocation: None,
                registry: Some(registry),
            }
        }
    }
}

enum EnterTargetError {
    Failed(String),
    DeferredEncryptedRelocation,
}

fn enter_target_with_source(
    source_dir: &Path,
    target: &Path,
) -> std::result::Result<(), EnterTargetError> {
    let target_db = target.join(DB_FILE_NAME);
    let source_db = source_dir.join(DB_FILE_NAME);

    if target_db.exists() {
        return Ok(());
    }
    if !source_db.exists() {
        return ensure_target_dir(target).map_err(EnterTargetError::Failed);
    }
    match probe_file_kind(&source_db) {
        Ok(DbFileKind::Encrypted) if super::encryption::has_encrypted_file_layout(&source_db) => {
            return Err(EnterTargetError::DeferredEncryptedRelocation);
        }
        Ok(_) => {}
        Err(e) => {
            return Err(EnterTargetError::Failed(format!(
                "原库无法探测（{}）：{e}",
                source_db.display()
            )));
        }
    }
    ensure_target_dir(target).map_err(EnterTargetError::Failed)?;
    relocate(&source_db, &target_db, None).map_err(EnterTargetError::Failed)
}

fn ensure_target_dir(target: &Path) -> std::result::Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|e| format!("目标目录不可用（无法创建 {}）：{e}", target.display()))
}

fn relocate(
    source_db: &Path,
    target_db: &Path,
    passphrase: Option<&str>,
) -> std::result::Result<(), String> {
    let open_by_key = |path: &Path| match passphrase {
        Some(pass) => open_connection_with_passphrase_unmounted(path, pass),
        None => open_connection_unmounted(path),
    };
    let source = open_by_key(source_db)
        .map_err(|e| format!("原库无法打开（{}）：{e}", source_db.display()))?;
    let tmp_db = temp_sibling(target_db, "relocate");
    let source_sync = sync_db_path(source_db);
    let target_sync = sync_db_path(target_db);
    let tmp_sync = temp_sibling(&target_sync, "relocate");
    let carries_sync = source_sync.exists();

    let result = (|| -> std::result::Result<(), String> {
        source
            .execute("VACUUM INTO ?1", params![tmp_db.to_string_lossy()])
            .map_err(|e| format!("整库搬迁失败（VACUUM INTO）：{e}"))?;
        let check = open_by_key(&tmp_db).map_err(|e| format!("搬迁临时库无法打开：{e}"))?;
        check_integrity(&check).map_err(|e| format!("搬迁临时库完整性检查失败：{e}"))?;
        if carries_sync {
            let sync_source = open_by_key(&source_sync)
                .map_err(|e| format!("同步元数据库无法打开（{}）：{e}", source_sync.display()))?;
            sync_source
                .execute("VACUUM INTO ?1", params![tmp_sync.to_string_lossy()])
                .map_err(|e| format!("同步元数据搬迁失败（VACUUM INTO）：{e}"))?;
            let check_sync =
                open_by_key(&tmp_sync).map_err(|e| format!("同步元数据搬迁临时库无法打开：{e}"))?;
            check_integrity(&check_sync)
                .map_err(|e| format!("同步元数据搬迁临时库完整性检查失败：{e}"))?;
            replace_file(&tmp_sync, &target_sync)
                .map_err(|e| format!("同步元数据搬迁临时库替换启用失败：{e}"))?;
        }
        replace_file(&tmp_db, target_db).map_err(|e| format!("搬迁临时库替换启用失败：{e}"))?;
        Ok(())
    })();

    if let Err(reason) = result {
        cleanup(&tmp_db);
        cleanup(&tmp_sync);
        return Err(reason);
    }
    Ok(())
}

pub fn relocate_with_key(source_db: &Path, target_db: &Path, passphrase: &str) -> Result<()> {
    relocate(source_db, target_db, Some(passphrase)).map_err(AppError::Io)
}

pub fn configured_intent(default_dir: &Path) -> Option<PathBuf> {
    match read_registry(default_dir) {
        RegistryRead::Resolved(registry) => registry.active_dir().map(std::path::Path::to_path_buf),
        RegistryRead::Unconfigured | RegistryRead::Corrupt(_) => None,
    }
}

pub fn write_pointer(default_dir: &Path, target: &Path) -> crate::error::Result<()> {
    std::fs::create_dir_all(default_dir)?;
    let pointer = default_dir.join(POINTER_FILE_NAME);
    let content = serde_json::to_string_pretty(&PointerIntent {
        data_dir: target.to_string_lossy().into_owned(),
    })?;
    atomic_write(&pointer, content.as_bytes())
}

#[derive(Serialize)]
struct PointerIntent {
    data_dir: String,
}

pub const WRITE_PROBE_FILE_NAME: &str = ".ledger_write_probe";

#[derive(Debug, Serialize)]
pub struct DataLocationChangeOutcome {
    pub requires_choice: bool,
    pub committed: bool,
    pub target_dir: Option<String>,
}

pub fn validate_and_commit(
    default_dir: &Path,
    target: &Path,
    adopt_existing: bool,
) -> Result<DataLocationChangeOutcome> {
    std::fs::create_dir_all(target).map_err(|e| {
        AppError::codedp(
            "data-location.mkdir-failed",
            format!("无法创建目标目录（{}）：{e}", target.display()),
            &[&target.display().to_string(), &e.to_string()],
        )
    })?;

    let probe = target.join(WRITE_PROBE_FILE_NAME);
    let probe_result = (|| -> std::io::Result<()> {
        std::fs::write(&probe, b"ok")?;
        std::fs::remove_file(&probe)
    })();
    probe_result.map_err(|e| {
        AppError::codedp(
            "data-location.dir-not-writable",
            format!("目标目录不可写（{}）：{e}", target.display()),
            &[&target.display().to_string(), &e.to_string()],
        )
    })?;

    let target_db = target.join(DB_FILE_NAME);
    if target_db.exists() && !adopt_existing {
        tracing::info!(target = %target.display(), "目标位置已有同名库，返回二选一信号");
        return Ok(DataLocationChangeOutcome {
            requires_choice: true,
            committed: false,
            target_dir: None,
        });
    }

    let mut registry = match read_registry(default_dir) {
        RegistryRead::Resolved(registry) => registry,
        RegistryRead::Unconfigured | RegistryRead::Corrupt(_) => {
            BookRegistry::single_default(default_dir)
        }
    };
    let Some(book) = registry.active() else {
        return Err(AppError::coded(
            "book.registry-unavailable",
            "账本注册表尚未就绪，请重启应用后再试",
        ));
    };
    let book_id = book.id.clone();
    let from_dir = book.dir.clone();
    if from_dir == target {
        book_registry::write_registry(default_dir, &registry)?;
        tracing::info!(target = %target.display(), "目标即当前位置，注册表已落盘（无搬迁）");
        return Ok(DataLocationChangeOutcome {
            requires_choice: false,
            committed: true,
            target_dir: Some(target.to_string_lossy().into_owned()),
        });
    }
    ensure_target_not_registered(&registry, &book_id, target)?;
    if let Some(book) = registry.active_mut() {
        book.dir = target.to_path_buf();
    }
    registry.pending_relocation = Some(book_registry::PendingRelocation { book_id, from_dir });
    book_registry::write_registry(default_dir, &registry)?;
    tracing::info!(target = %target.display(), "活动账本搬迁意图已落盘，下次启动生效");
    Ok(DataLocationChangeOutcome {
        requires_choice: false,
        committed: true,
        target_dir: Some(target.to_string_lossy().into_owned()),
    })
}

fn ensure_target_not_registered(
    registry: &BookRegistry,
    active_id: &str,
    target: &Path,
) -> Result<()> {
    let canonical = target.canonicalize()?;
    for book in &registry.books {
        if book.id == active_id {
            continue;
        }
        let existing = book.dir.canonicalize().unwrap_or_else(|_| book.dir.clone());
        if existing == canonical {
            return Err(AppError::codedp(
                "book.dir-exists",
                format!("该目录已登记为账本「{}」", book.name),
                &[&book.name],
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct DataLocationInfo {
    pub active_dir: String,
    pub configured_dir: Option<String>,
    pub pending_restart: bool,
    pub fallback_reason: Option<String>,
}

pub fn gather_info_from_boot(default_dir: &Path, boot: Option<&Boot>) -> DataLocationInfo {
    let (active_dir, fallback) = match boot {
        Some(boot) => (boot.db_dir.clone(), boot.fallback_reason.clone()),
        None => (default_dir.to_path_buf(), None),
    };
    gather_info(default_dir, &active_dir, fallback.as_deref())
}

pub fn effective_db_dir(boot: Option<&Boot>, default_dir: &Path) -> PathBuf {
    boot.map(|boot| boot.db_dir.clone())
        .unwrap_or_else(|| default_dir.to_path_buf())
}

pub fn gather_info(
    default_dir: &Path,
    active_dir: &Path,
    fallback_reason: Option<&str>,
) -> DataLocationInfo {
    let configured_dir = configured_intent(default_dir);
    let pending_restart = match &configured_dir {
        Some(intent) => intent != active_dir,
        None => false,
    };
    DataLocationInfo {
        active_dir: active_dir.to_string_lossy().into_owned(),
        configured_dir: configured_dir.map(|dir| dir.to_string_lossy().into_owned()),
        pending_restart,
        fallback_reason: fallback_reason.map(str::to_string),
    }
}

pub fn mutable_registry(boot: Option<&Boot>) -> Result<&BookRegistry> {
    let Some(boot) = boot else {
        return Err(AppError::coded(
            "book.registry-unavailable",
            "账本注册表尚未就绪，请重启应用后再试",
        ));
    };
    if boot.deferred_relocation.is_some() {
        return Err(book_registry::registry_busy_error());
    }
    boot.registry.as_ref().ok_or_else(|| {
        let reason = boot
            .fallback_reason
            .clone()
            .unwrap_or_else(|| "注册表不可读".into());
        book_registry::registry_corrupt_error(&reason)
    })
}

pub fn ensure_relocation_settled(boot: Option<&Boot>) -> Result<()> {
    if boot.is_some_and(|boot| boot.deferred_relocation.is_some()) {
        return Err(book_registry::registry_busy_error());
    }
    Ok(())
}

pub fn gather_book_list(default_dir: &Path, boot: Option<&Boot>) -> book_registry::BookListInfo {
    let (books, active_id, registry_ok) = match book_registry::read_registry(default_dir) {
        RegistryRead::Resolved(registry) => (registry.books, Some(registry.active_id), true),
        RegistryRead::Unconfigured => {
            let registry = BookRegistry::single_default(default_dir);
            (registry.books, Some(registry.active_id), true)
        }
        RegistryRead::Corrupt(_) => (Vec::new(), None, false),
    };
    let (mutable, fallback_reason) = match boot {
        Some(boot) => (
            registry_ok && mutable_registry(Some(boot)).is_ok(),
            boot.fallback_reason.clone(),
        ),
        None => (false, None),
    };
    book_registry::BookListInfo {
        books,
        active_id,
        mutable,
        fallback_reason,
    }
}
