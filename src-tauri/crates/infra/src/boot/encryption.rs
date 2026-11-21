use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::fs_util::{cleanup, temp_sibling};

pub const SQLITE_HEADER_MAGIC: [u8; 16] = *b"SQLite format 3\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbFileKind {
    Plaintext,
    Encrypted,
    Empty,
}

pub fn probe_file_kind(path: &Path) -> Result<DbFileKind> {
    Ok(probe_file(path)?.kind)
}

pub const HEADER_RESERVED_BYTES_OFFSET: usize = 20;

const HEADER_PROBE_BYTES: usize = HEADER_RESERVED_BYTES_OFFSET + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DbFileProbe {
    pub kind: DbFileKind,
    pub reserved_bytes: Option<u8>,
}

pub fn probe_file(path: &Path) -> Result<DbFileProbe> {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DbFileProbe {
                kind: DbFileKind::Empty,
                reserved_bytes: None,
            });
        }
        Err(e) => return Err(e.into()),
    };
    let mut header = [0u8; HEADER_PROBE_BYTES];
    let mut filled = 0;
    while filled < header.len() {
        let n = file.read(&mut header[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    if filled < SQLITE_HEADER_MAGIC.len() {
        return Ok(DbFileProbe {
            kind: DbFileKind::Empty,
            reserved_bytes: None,
        });
    }
    if header[..SQLITE_HEADER_MAGIC.len()] != SQLITE_HEADER_MAGIC {
        return Ok(DbFileProbe {
            kind: DbFileKind::Encrypted,
            reserved_bytes: None,
        });
    }
    Ok(DbFileProbe {
        kind: DbFileKind::Plaintext,
        reserved_bytes: (filled > HEADER_RESERVED_BYTES_OFFSET)
            .then(|| header[HEADER_RESERVED_BYTES_OFFSET]),
    })
}

const SQLCIPHER_DEFAULT_PAGE_SIZE: u64 = 4096;

pub fn has_encrypted_file_layout(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let size = meta.len();
            size >= SQLCIPHER_DEFAULT_PAGE_SIZE && size % SQLCIPHER_DEFAULT_PAGE_SIZE == 0
        }
        Err(_) => false,
    }
}

#[derive(Clone)]
pub struct EncryptionGate {
    locked: Arc<AtomicBool>,
}

impl EncryptionGate {
    pub fn new(locked: bool) -> Self {
        Self {
            locked: Arc::new(AtomicBool::new(locked)),
        }
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::SeqCst);
    }
}

pub fn enable_encryption_for_file(db_path: &Path, passphrase: &str) -> Result<()> {
    if passphrase.is_empty() {
        return Err(AppError::coded(
            "encryption.passphrase-empty",
            "主口令不能为空",
        ));
    }
    match probe_file_kind(db_path)? {
        DbFileKind::Plaintext => {}
        DbFileKind::Empty => {
            return Err(AppError::coded(
                "encryption.db-missing",
                "数据库文件不存在或为空",
            ));
        }
        DbFileKind::Encrypted => {
            return Err(AppError::coded(
                "encryption.not-plaintext",
                "当前库已是加密库，无需再次开启加密",
            ));
        }
    }
    convert_db_file(db_path, None, Some(passphrase))
}

pub fn disable_encryption_for_file(db_path: &Path, current_passphrase: &str) -> Result<()> {
    require_encrypted_file(db_path)?;
    verify_source_passphrase(db_path, current_passphrase)?;
    convert_db_file(db_path, Some(current_passphrase), None)
}

pub fn change_passphrase_for_file(
    db_path: &Path,
    current_passphrase: &str,
    new_passphrase: &str,
) -> Result<()> {
    if new_passphrase.is_empty() {
        return Err(AppError::coded(
            "encryption.passphrase-empty",
            "主口令不能为空",
        ));
    }
    require_encrypted_file(db_path)?;
    verify_source_passphrase(db_path, current_passphrase)?;
    convert_db_file(db_path, Some(current_passphrase), Some(new_passphrase))
}

pub fn normalize_plaintext_db_file(db_path: &Path) -> Result<()> {
    match probe_file(db_path)?.reserved_bytes {
        Some(reserved) if reserved != 0 => {
            convert_db_file(db_path, None, None)?;
            tracing::info!(
                reserved_bytes = reserved,
                "外来形态明文库已归一化（每页保留字节归零）"
            );
            Ok(())
        }
        Some(_) => Ok(()),
        None => Err(AppError::Io(format!(
            "外来形态归一化仅适用于明文库: {}",
            db_path.display()
        ))),
    }
}

fn convert_db_file(
    db_path: &Path,
    source_passphrase: Option<&str>,
    target_passphrase: Option<&str>,
) -> Result<()> {
    let sync_path = crate::db::sync_db_path(db_path);
    let sync_exists = sync_path.exists();
    let tmp_path = temp_sibling(db_path, "convert");
    let sync_tmp = sync_exists.then(|| temp_sibling(&sync_path, "convert"));
    let result = (|| -> Result<()> {
        let user_version =
            export_converted_copy(db_path, &tmp_path, source_passphrase, target_passphrase)?;
        let sync_user_version = match &sync_tmp {
            Some(path) => Some(export_converted_copy(
                &sync_path,
                path,
                source_passphrase,
                target_passphrase,
            )?),
            None => None,
        };
        verify_converted_copy(&tmp_path, target_passphrase, user_version)?;
        if let (Some(path), Some(version)) = (&sync_tmp, sync_user_version) {
            verify_converted_copy(path, target_passphrase, version)?;
        }
        promote_converted_copy(db_path, &tmp_path)?;
        if let Some(path) = &sync_tmp
            && let Err(error) = promote_converted_copy(&sync_path, path)
        {
            rollback_promoted_copy(db_path);
            return Err(error);
        }
        Ok(())
    })();

    if let Err(error) = result {
        cleanup(&tmp_path);
        if let Some(path) = &sync_tmp {
            cleanup(path);
        }
        tracing::warn!(error = %error, "整库转换失败，原库保持原样不变");
        return Err(error);
    }
    tracing::info!(bak = %bak_path(db_path).display(), "整库转换完成，原库保留为 .bak 副本");
    Ok(())
}

fn rollback_promoted_copy(db_path: &Path) {
    let bak = bak_path(db_path);
    if !bak.exists() {
        tracing::error!("转换回滚失败：找不到 .bak 原库副本，保持现场");
        return;
    }
    let aside = temp_sibling(db_path, "rollback");
    match std::fs::rename(db_path, &aside) {
        Ok(()) => {
            if let Err(e) = std::fs::rename(&bak, db_path) {
                let _ = std::fs::rename(&aside, db_path);
                tracing::error!(error = %e, "转换回滚失败：原库副本归位失败，已恢复转换副本");
                return;
            }
            cleanup(&aside);
            tracing::info!(bak = %bak.display(), "转换回滚完成：原库副本已归位");
        }
        Err(e) => {
            tracing::error!(error = %e, "转换回滚失败：转换副本未能移开，保持现场");
        }
    }
}

fn require_encrypted_file(db_path: &Path) -> Result<()> {
    match probe_file_kind(db_path)? {
        DbFileKind::Encrypted => Ok(()),
        DbFileKind::Empty => Err(AppError::coded(
            "encryption.db-missing",
            "数据库文件不存在或为空",
        )),
        DbFileKind::Plaintext => Err(AppError::coded(
            "encryption.not-encrypted",
            "库文件当前不是加密状态，请重启应用后再试",
        )),
    }
}

pub fn verify_source_passphrase(db_path: &Path, passphrase: &str) -> Result<()> {
    verify_main_passphrase_bare(db_path, passphrase)
}

fn verify_main_passphrase_bare(db_path: &Path, passphrase: &str) -> Result<()> {
    let conn = Connection::open(db_path)?;
    conn.busy_timeout(crate::db::CONCURRENT_BUSY_TIMEOUT)?;
    conn.pragma_update(None, "key", passphrase)?;
    match conn.query_row::<i64, _, _>("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    }) {
        Ok(_) => Ok(()),
        Err(e) if is_not_a_database(&e) => Err(passphrase_incorrect_error()),
        Err(e) => Err(e.into()),
    }
}

fn bak_path(db_path: &Path) -> std::path::PathBuf {
    db_path.with_extension("db.bak")
}

fn export_converted_copy(
    source: &Path,
    target: &Path,
    source_passphrase: Option<&str>,
    target_passphrase: Option<&str>,
) -> Result<i64> {
    let conn = Connection::open(source)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    if let Some(passphrase) = source_passphrase {
        conn.pragma_update(None, "key", passphrase)?;
    }
    let user_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let target_key = match target_passphrase {
        Some(pass) => crate::db::connection::sql_string_literal(pass),
        None => String::from("''"),
    };
    let attach_sql = format!(
        "ATTACH DATABASE {} AS encryption_target KEY {target_key}",
        crate::db::connection::sql_string_literal(&target.to_string_lossy()),
    );
    conn.execute_batch(&attach_sql)?;
    let export_result = (|| -> Result<()> {
        conn.execute_batch("SELECT sqlcipher_export('encryption_target')")?;
        conn.execute_batch(&format!(
            "PRAGMA encryption_target.user_version = {user_version}"
        ))?;
        Ok(())
    })();
    let result = export_result.and_then(|()| {
        conn.execute_batch("DETACH DATABASE encryption_target")
            .map_err(Into::into)
    });
    result?;
    Ok(user_version)
}

fn verify_converted_copy(target: &Path, passphrase: Option<&str>, user_version: i64) -> Result<()> {
    let conn = match passphrase {
        Some(pass) => crate::db::open_connection_with_passphrase(target, pass)?,
        None => crate::db::open_connection(target)?,
    };
    crate::db::check_integrity(&conn)
        .map_err(|e| AppError::Io(format!("转换副本完整性检查失败: {e}")))?;
    let copied: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if copied != user_version {
        return Err(AppError::Io(format!(
            "转换副本 schema 版本不一致（{copied} ≠ {user_version}），已拒绝启用"
        )));
    }
    Ok(())
}

fn promote_converted_copy(db_path: &Path, tmp_path: &Path) -> Result<()> {
    let bak = bak_path(db_path);
    std::fs::rename(db_path, &bak)?;
    if let Err(e) = std::fs::rename(tmp_path, db_path) {
        std::fs::rename(&bak, db_path).ok();
        return Err(AppError::Io(format!("转换副本替换启用失败: {e}")));
    }
    Ok(())
}

pub fn reset_encrypted_db_file(db_path: &Path) -> Result<Connection> {
    match probe_file_kind(db_path)? {
        DbFileKind::Encrypted => {}
        DbFileKind::Empty => {
            return Err(AppError::coded(
                "encryption.db-missing",
                "数据库文件不存在或为空",
            ));
        }
        DbFileKind::Plaintext => {
            return Err(AppError::coded(
                "encryption.not-encrypted",
                "库文件当前不是加密状态，请重启应用后再试",
            ));
        }
    }
    let sync_path = crate::db::sync_db_path(db_path);
    if sync_path.exists() {
        std::fs::rename(&sync_path, bak_path(&sync_path))?;
    }
    let bak = bak_path(db_path);
    std::fs::rename(db_path, &bak)?;
    match open_new_plaintext_db(db_path) {
        Ok(conn) => {
            tracing::info!(bak = %bak.display(), "忘记口令重置完成：原密文库保留为 .bak 副本，新明文空库就绪");
            Ok(conn)
        }
        Err(e) => {
            std::fs::rename(&bak, db_path).ok();
            std::fs::rename(bak_path(&sync_path), &sync_path).ok();
            tracing::error!(error = %e, "忘记口令重置失败，原密文库已改回原位");
            Err(e)
        }
    }
}

fn open_new_plaintext_db(db_path: &Path) -> Result<Connection> {
    let mut conn = crate::db::open_connection(db_path)?;
    crate::db::init_db(&mut conn)?;
    crate::db::check_integrity(&conn)?;
    Ok(conn)
}

pub fn passphrase_incorrect_error() -> AppError {
    crate::db::connection::passphrase_incorrect_error()
}

pub fn unlock_db_file(db_path: &Path, passphrase: &str) -> Result<Connection> {
    match probe_file_kind(db_path)? {
        DbFileKind::Encrypted => {}
        DbFileKind::Empty => {
            return Err(AppError::coded(
                "encryption.db-missing",
                "数据库文件不存在或为空",
            ));
        }
        DbFileKind::Plaintext => {
            return Err(AppError::coded(
                "encryption.not-encrypted",
                "库文件当前不是加密状态，请重启应用后再试",
            ));
        }
    }
    verify_main_passphrase_bare(db_path, passphrase)?;
    let conn = crate::db::open_connection_with_passphrase(db_path, passphrase)?;
    let mut conn = conn;
    crate::db::init_db(&mut conn)?;
    if let Err(e) = crate::db::check_integrity(&conn) {
        tracing::error!(error = %e, "密文库完整性检查未通过");
        return Err(AppError::coded(
            "encryption.db-corrupt",
            "数据库文件损坏，无法通过完整性检查",
        ));
    }
    Ok(conn)
}

pub fn is_not_a_database(e: &rusqlite::Error) -> bool {
    crate::db::connection::is_not_a_database_error(e)
}
