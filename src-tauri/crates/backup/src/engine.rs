use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::db;
use ledger_infra::db::encryption::DbFileKind;
use ledger_infra::error::{AppError, Result};
use ledger_infra::fs_util::{cleanup, replace_file, temp_sibling};

use crate::identity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupScope {
    pub fingerprint: String,
    pub device_id: Option<String>,
}

impl BackupScope {
    pub fn of_conn(conn: &Connection) -> Result<Option<Self>> {
        Ok(
            identity::book_fingerprint_readonly(conn)?.map(|fingerprint| Self {
                fingerprint,
                device_id: identity::source_device_id(conn),
            }),
        )
    }

    pub(crate) fn book_tag(&self) -> &str {
        &self.fingerprint
    }
}

pub(crate) const ZIP_DB_ENTRY: &str = "ledger.db";
const ZIP_SYNC_ENTRY: &str = "sync.db";
const ZIP_META_ENTRY: &str = "backup.json";

const MANAGED_BACKUP_PREFIXES: &[&str] = &["ledger-backup-", super::auto::AUTO_BACKUP_PREFIX];
const MANAGED_BACKUP_SUFFIX: &str = ".db.zip";

const TIMESTAMP_SEGMENT_LEN: usize = 15;

pub(crate) fn managed_backup_file_name(
    prefix: &str,
    timestamp: &str,
    book: Option<&str>,
) -> String {
    match book {
        Some(id) => format!("{prefix}{timestamp}-{id}{MANAGED_BACKUP_SUFFIX}"),
        None => format!("{prefix}{timestamp}{MANAGED_BACKUP_SUFFIX}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub enum BackupKind {
    #[serde(rename = "auto")]
    Auto,
    #[default]
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "pre-update")]
    PreUpdate,
}

impl std::fmt::Display for BackupKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Manual => f.write_str("manual"),
            Self::PreUpdate => f.write_str("pre-update"),
        }
    }
}

impl<'de> Deserialize<'de> for BackupKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "auto" {
            Ok(Self::Auto)
        } else if s == "pre-update" {
            Ok(Self::PreUpdate)
        } else {
            Ok(Self::Manual)
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BackupFileInfo {
    pub file_name: String,
    pub path: String,
    pub size_bytes: u64,
    pub created_at: String,
    pub kind: BackupKind,
    pub encrypted: bool,
    pub source_device_id: Option<String>,
    pub source_unknown: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BackupMetaSummary {
    pub kind: BackupKind,
    pub encrypted: bool,
}

#[derive(Debug, Serialize)]
pub struct PruneResult {
    pub kept: usize,
    pub deleted: Vec<String>,
    pub failed: Vec<String>,
}

fn matched_managed_prefix(name: &str) -> Option<&'static str> {
    MANAGED_BACKUP_PREFIXES
        .iter()
        .copied()
        .find(|prefix| name.starts_with(prefix))
}

pub(crate) fn is_managed_backup_file_name(name: &str) -> bool {
    matched_managed_prefix(name).is_some() && name.ends_with(MANAGED_BACKUP_SUFFIX)
}

pub(crate) fn open_backup_archive(backup_path: &Path) -> Result<zip::ZipArchive<File>> {
    let file = File::open(backup_path)?;
    zip::ZipArchive::new(file).map_err(|e| {
        AppError::codedp(
            "backup.invalid-archive",
            format!("不是有效的开源记账备份包: {e}"),
            &[&e.to_string()],
        )
    })
}

pub(crate) fn read_meta_from_archive(archive: &mut zip::ZipArchive<File>) -> Result<BackupMeta> {
    let mut entry = archive.by_name(ZIP_META_ENTRY).map_err(|_| {
        AppError::coded(
            "backup.meta-missing",
            format!("备份包内未找到 {} 元数据", ZIP_META_ENTRY),
        )
    })?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    serde_json::from_slice::<BackupMeta>(&buf).map_err(|e| {
        AppError::codedp(
            "backup.meta-parse-failed",
            format!("备份元数据解析失败: {e}"),
            &[&e.to_string()],
        )
    })
}

pub fn read_backup_meta(backup_path: &Path) -> Result<BackupMetaSummary> {
    let mut archive = open_backup_archive(backup_path)?;
    let meta = read_meta_from_archive(&mut archive)?;
    Ok(BackupMetaSummary {
        kind: meta.kind,
        encrypted: meta.encrypted,
    })
}

pub fn probe_backup_meta(backup_path: &Path) -> Result<BackupMetaSummary> {
    match open_backup_archive(backup_path) {
        Ok(mut archive) => {
            let meta = read_meta_from_archive(&mut archive)?;
            Ok(BackupMetaSummary {
                kind: meta.kind,
                encrypted: meta.encrypted,
            })
        }
        Err(_) => {
            let encrypted = matches!(
                db::encryption::probe_file_kind(backup_path)?,
                DbFileKind::Encrypted
            );
            Ok(BackupMetaSummary {
                kind: BackupKind::Manual,
                encrypted,
            })
        }
    }
}

pub fn read_backup_kind(backup_path: &Path) -> Result<BackupKind> {
    read_backup_meta(backup_path).map(|s| s.kind)
}

pub(crate) fn split_managed_name(file_name: &str) -> Option<(NaiveDateTime, Option<&str>)> {
    let prefix = matched_managed_prefix(file_name)?;
    let stem = file_name
        .strip_prefix(prefix)?
        .strip_suffix(MANAGED_BACKUP_SUFFIX)?;
    if stem.len() == TIMESTAMP_SEGMENT_LEN {
        let ts = NaiveDateTime::parse_from_str(stem, "%Y%m%d-%H%M%S").ok()?;
        return Some((ts, None));
    }
    if stem.len() > TIMESTAMP_SEGMENT_LEN + 1 && stem.as_bytes()[TIMESTAMP_SEGMENT_LEN] == b'-' {
        let ts =
            NaiveDateTime::parse_from_str(&stem[..TIMESTAMP_SEGMENT_LEN], "%Y%m%d-%H%M%S").ok()?;
        let book = &stem[TIMESTAMP_SEGMENT_LEN + 1..];
        if !book.is_empty() {
            return Some((ts, Some(book)));
        }
    }
    None
}

fn parse_backup_timestamp(file_name: &str) -> Option<NaiveDateTime> {
    split_managed_name(file_name).map(|(ts, _)| ts)
}

fn collect_scoped_in_dir(
    root: &Path,
    key_prefix: &str,
    scope_and_cache: &mut Option<(&BackupScope, crate::attribution::AttributionCache)>,
    files: &mut Vec<(
        NaiveDateTime,
        PathBuf,
        Option<crate::attribution::ProductMeta>,
    )>,
) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if !is_managed_backup_file_name(&name) || !is_file {
            continue;
        }
        let file_book = split_managed_name(&name).and_then(|(_, book)| book);
        let attributed = match scope_and_cache.as_mut() {
            None => None,
            Some((scope, cache)) => {
                match crate::attribution::attribute_to_scope(
                    &format!("{key_prefix}{name}"),
                    &entry.path(),
                    file_book,
                    scope,
                    cache,
                ) {
                    crate::attribution::Attribution::Foreign => continue,
                    crate::attribution::Attribution::Owned(meta)
                    | crate::attribution::Attribution::Unknown(meta) => meta,
                }
            }
        };
        let ts = parse_backup_timestamp(&name).or_else(|| {
            entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(chrono::DateTime::<chrono::Utc>::from)
                .map(|t| t.naive_utc())
        });
        files.push((ts.unwrap_or(NaiveDateTime::MIN), entry.path(), attributed));
    }
    Ok(())
}

pub fn managed_write_target(dir: &Path, file_name: &str, conn: &Connection) -> Result<PathBuf> {
    let target = match identity::source_device_id(conn) {
        Some(device) => {
            let device_dir = dir.join(&device);
            if dir.is_dir() {
                std::fs::create_dir_all(&device_dir)?;
            }
            device_dir.join(file_name)
        }
        None => dir.join(file_name),
    };
    Ok(target)
}

pub fn managed_command_target(target: &Path, conn: &Connection) -> Result<PathBuf> {
    let Some(name) = target.file_name().and_then(|n| n.to_str()) else {
        return Ok(target.to_path_buf());
    };
    if !is_managed_backup_file_name(name) {
        return Ok(target.to_path_buf());
    }
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Ok(target.to_path_buf()),
    };
    let Some(mirror) = super::auto::shared_prefs().snapshot_dir() else {
        return Ok(target.to_path_buf());
    };
    if !same_dir_path(&mirror, &parent.to_string_lossy()) {
        return Ok(target.to_path_buf());
    }
    managed_write_target(parent, name, conn)
}

fn same_dir_path(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> &str {
        s.trim_end_matches(['/', '\\'])
    }
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && a == b
}

pub fn list_managed_backups(
    dir: &Path,
    scope: Option<&BackupScope>,
) -> Result<Vec<BackupFileInfo>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut scope_and_cache = scope.map(|s| (s, crate::attribution::AttributionCache::new(dir)));
    let mut files: Vec<(
        NaiveDateTime,
        PathBuf,
        Option<crate::attribution::ProductMeta>,
    )> = Vec::new();
    collect_scoped_in_dir(dir, "", &mut scope_and_cache, &mut files)?;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let sub = entry.path();
        let prefix = format!("{}/", entry.file_name().to_string_lossy());
        if let Err(e) = collect_scoped_in_dir(&sub, &prefix, &mut scope_and_cache, &mut files) {
            tracing::debug!(dir = %sub.display(), error = %e, "子目录扫描失败，跳过");
        }
    }
    if let Some((_, cache)) = scope_and_cache.as_mut() {
        cache.flush();
    }
    files.sort_by_key(|a| std::cmp::Reverse(a.0));
    files
        .into_iter()
        .map(|(ts, path, attributed)| {
            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let (kind, encrypted, source_device_id) = match attributed {
                Some(meta) => (meta.kind, meta.encrypted, meta.device_id),
                None => match crate::attribution::read_product_meta(&path) {
                    Some(meta) => (meta.kind, meta.encrypted, meta.device_id),
                    None => {
                        if file_name.starts_with(super::auto::AUTO_BACKUP_PREFIX) {
                            (BackupKind::Auto, false, None)
                        } else {
                            (BackupKind::Manual, false, None)
                        }
                    }
                },
            };
            let source_unknown = source_device_id.is_none();
            Ok(BackupFileInfo {
                file_name,
                path: path.to_string_lossy().into_owned(),
                size_bytes,
                created_at: ts.format("%Y-%m-%dT%H:%M:%S").to_string(),
                kind,
                encrypted,
                source_device_id,
                source_unknown,
            })
        })
        .collect()
}

pub fn prune_managed_backups(
    dir: &Path,
    keep: usize,
    scope: Option<&BackupScope>,
) -> Result<PruneResult> {
    let files = list_managed_backups(dir, scope)?;
    let own_device = scope.and_then(|s| s.device_id.as_deref());
    let mut bucket: Vec<BackupFileInfo> = Vec::new();
    for file in files {
        let in_bucket = match own_device {
            Some(own) => file.source_device_id.as_deref() == Some(own),
            None => file.source_device_id.is_some(),
        };
        if in_bucket {
            bucket.push(file);
        } else {
            tracing::debug!(
                file = %file.file_name,
                "他端设备名下/来源未知的受管产物不在本桶，不参与本次清理（互不代删，ADR-0144 决策 4）"
            );
        }
    }
    let bucket_len = bucket.len();
    if bucket_len <= keep {
        return Ok(PruneResult {
            kept: bucket_len,
            deleted: Vec::new(),
            failed: Vec::new(),
        });
    }
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    let excess = bucket_len - keep;
    for file in bucket.into_iter().rev().take(excess) {
        match std::fs::remove_file(&file.path) {
            Ok(()) => {
                tracing::info!(file = %file.file_name, "已清理旧备份");
                deleted.push(file.file_name);
            }
            Err(e) => {
                tracing::warn!(file = %file.file_name, error = %e, "清理旧备份失败");
                failed.push(file.file_name);
            }
        }
    }
    Ok(PruneResult {
        kept: bucket_len - deleted.len(),
        deleted,
        failed,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackupMeta {
    pub(crate) created_at: String,
    pub(crate) app_version: String,
    pub(crate) schema_version: i64,
    #[serde(default)]
    pub(crate) kind: BackupKind,
    #[serde(default)]
    pub(crate) encrypted: bool,
    #[serde(default)]
    pub(crate) paired: bool,
    #[serde(default)]
    pub(crate) fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) device_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BackupResult {
    pub path: String,
    pub size_bytes: u64,
    pub schema_version: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub schema_version: i64,
    pub restored_at: String,
}

pub fn expected_schema_version() -> Result<i64> {
    let mut conn = db::open_in_memory()?;
    db::init_db(&mut conn)?;
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map_err(AppError::from)
}

fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map_err(AppError::from)
}

fn ensure_source_snapshotable(conn: &Connection) -> Result<()> {
    let path = match conn.path() {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(()),
    };
    let probe = db::encryption::probe_file(Path::new(path))?;
    if probe.kind == DbFileKind::Plaintext && probe.reserved_bytes.is_some_and(|v| v != 0) {
        return Err(AppError::coded(
            "backup.foreign-form",
            "库文件被外部工具改写，无法创建备份，重启应用可自动修复",
        ));
    }
    Ok(())
}

pub fn backup_db_to(
    conn: &Connection,
    target: &Path,
    app_version: &str,
    kind: BackupKind,
) -> Result<BackupResult> {
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if !parent.is_dir() {
        return Err(AppError::codedp(
            "backup.target-dir-missing",
            format!("备份目标目录不存在: {}", parent.display()),
            &[&parent.display().to_string()],
        ));
    }

    ensure_source_snapshotable(conn)?;

    let tmp_db = temp_sibling(target, "db");
    let tmp_sync = temp_sibling(target, "sync");
    let tmp_zip = temp_sibling(target, "zip");

    let result = (|| -> Result<BackupResult> {
        let fingerprint = identity::book_fingerprint(conn)?;
        let device_id = identity::source_device_id(conn);
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![tmp_db.to_string_lossy()],
        )?;
        let paired = true;
        db::snapshot_sync_tables_into(conn, &tmp_sync)?;

        let encrypted = matches!(
            db::encryption::probe_file_kind(&tmp_db)?,
            DbFileKind::Encrypted
        );
        let meta = BackupMeta {
            created_at: db::now_iso(),
            app_version: app_version.to_string(),
            schema_version: schema_version(conn)?,
            kind,
            encrypted,
            paired,
            fingerprint: Some(fingerprint),
            device_id,
        };
        let file = File::create(&tmp_zip)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(ZIP_DB_ENTRY, options)?;
        let mut db_file = File::open(&tmp_db)?;
        std::io::copy(&mut db_file, &mut zip)?;
        zip.start_file(ZIP_SYNC_ENTRY, options)?;
        let mut sync_file = File::open(&tmp_sync)?;
        std::io::copy(&mut sync_file, &mut zip)?;
        zip.start_file(ZIP_META_ENTRY, options)?;
        zip.write_all(serde_json::to_string_pretty(&meta)?.as_bytes())?;
        zip.finish()?;

        replace_file(&tmp_zip, target)?;

        let size_bytes = std::fs::metadata(target)?.len();
        tracing::info!(target = %target.display(), size = %size_bytes, schema = %meta.schema_version, paired, "备份完成");
        Ok(BackupResult {
            path: target.to_string_lossy().into_owned(),
            size_bytes,
            schema_version: meta.schema_version,
            created_at: meta.created_at,
        })
    })();
    cleanup(&tmp_db);
    cleanup(&tmp_sync);
    cleanup(&tmp_zip);
    result
}

pub const PRE_UPDATE_BACKUP_FILE_NAME: &str = "pre-update.db.zip";

pub fn create_pre_update_backup(
    conn: &Connection,
    dir: &Path,
    app_version: &str,
) -> Result<BackupResult> {
    backup_db_to(
        conn,
        &dir.join(PRE_UPDATE_BACKUP_FILE_NAME),
        app_version,
        BackupKind::PreUpdate,
    )
}

pub fn restore_db_from(
    backup_path: &Path,
    db_path: &Path,
    safety_dir: &Path,
    expected_schema: i64,
    passphrase: Option<&str>,
) -> Result<RestoreResult> {
    let tmp_db = temp_sibling(db_path, "restore");
    let tmp_sync = temp_sibling(db_path, "restore-sync");

    let sync_entry_present = match extract_db_files(backup_path, &tmp_db, &tmp_sync) {
        Ok(found) => found,
        Err(e) => {
            cleanup(&tmp_db);
            cleanup(&tmp_sync);
            return Err(e);
        }
    };
    let meta_paired = backup_paired_flag(backup_path);
    let paired = meta_paired && sync_entry_present;
    if meta_paired && !sync_entry_present {
        cleanup(&tmp_db);
        cleanup(&tmp_sync);
        return Err(AppError::coded(
            "backup.sync-entry-missing",
            format!("备份包内未找到 {ZIP_SYNC_ENTRY}，成对备份不完整"),
        ));
    }

    let backup_encrypted = match db::encryption::probe_file_kind(&tmp_db) {
        Ok(kind) => kind == DbFileKind::Encrypted,
        Err(e) => {
            cleanup(&tmp_db);
            cleanup(&tmp_sync);
            return Err(e);
        }
    };

    let backup_schema = match validate_backup(&tmp_db, expected_schema, passphrase) {
        Ok(v) => v,
        Err(e) => {
            cleanup(&tmp_db);
            cleanup(&tmp_sync);
            return Err(e);
        }
    };
    if paired && let Err(e) = validate_backup_sync_component(&tmp_sync, passphrase) {
        cleanup(&tmp_db);
        cleanup(&tmp_sync);
        return Err(e);
    }

    let mut sync_moved: Option<(std::path::PathBuf, std::path::PathBuf)> = None;
    if db_path.exists() {
        std::fs::create_dir_all(safety_dir)?;
        let stamp = db::now_iso().replace([':', 'T'], "-");
        let safety = safety_dir.join(format!("restore-safety-{stamp}.db"));
        std::fs::copy(db_path, &safety)?;
        let sync_path = db::sync_db_path(db_path);
        if sync_path.exists() {
            let sync_safety = safety_dir.join(format!("restore-safety-sync-{stamp}.db"));
            std::fs::rename(&sync_path, &sync_safety)?;
            tracing::info!(safety = %sync_safety.display(), "恢复前已移位当前同步元数据库");
            sync_moved = Some((sync_path, sync_safety));
        }
        tracing::info!(safety = %safety.display(), "恢复前已自动备份当前数据库");
    }

    let sync_path = db::sync_db_path(db_path);
    if paired && let Err(error) = replace_file(&tmp_sync, &sync_path) {
        if let Some((sync_path, sync_safety)) = &sync_moved
            && let Err(move_back) = std::fs::rename(sync_safety, sync_path)
        {
            tracing::error!(
                error = %move_back,
                "恢复 sync.db 替换失败后归位失败，保持移位现场"
            );
        }
        cleanup(&tmp_db);
        cleanup(&tmp_sync);
        return Err(error);
    }
    cleanup(&tmp_sync);
    let replace_result = replace_file(&tmp_db, db_path);
    cleanup(&tmp_db);
    if let Err(error) = replace_result {
        if let Some((sync_path, sync_safety)) = &sync_moved
            && let Err(move_back) = std::fs::rename(sync_safety, sync_path)
        {
            tracing::error!(
                error = %move_back,
                "恢复替换失败后同步元数据库归位失败，保持移位现场"
            );
        }
        return Err(error);
    }

    let restored_at = db::now_iso();
    let open_reset = match (backup_encrypted, passphrase) {
        (true, Some(pass)) => db::open_connection_with_passphrase(db_path, pass),
        (false, _) => db::open_connection(db_path),
        (true, None) => {
            tracing::warn!("恢复出的密文库缺少主口令，跳过自动备份状态重置");
            tracing::info!(schema = %backup_schema, "恢复完成");
            return Ok(RestoreResult {
                schema_version: backup_schema,
                restored_at,
            });
        }
    };
    match open_reset {
        Ok(conn) => {
            if let Err(e) = super::auto::reset(&conn, &restored_at) {
                tracing::warn!(error = %e, "恢复后重置自动备份状态失败");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "恢复后打开新库重置自动备份状态失败");
        }
    }
    tracing::info!(schema = %backup_schema, paired, "恢复完成");
    Ok(RestoreResult {
        schema_version: backup_schema,
        restored_at,
    })
}

fn open_backup_db_conn(tmp_db: &Path, passphrase: Option<&str>) -> Result<Connection> {
    match db::encryption::probe_file_kind(tmp_db)? {
        DbFileKind::Encrypted => {
            let conn = match passphrase {
                Some(pass) => db::open_connection_with_passphrase(tmp_db, pass)?,
                None => {
                    return Err(AppError::coded(
                        "backup.passphrase-required",
                        "该备份为密文备份，需要备份所在库的主口令才能恢复",
                    ));
                }
            };
            if let Err(e) =
                conn.query_row::<i64, _, _>("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
            {
                if db::encryption::is_not_a_database(&e) {
                    return Err(db::encryption::passphrase_incorrect_error());
                }
                return Err(e.into());
            }
            Ok(conn)
        }
        DbFileKind::Plaintext | DbFileKind::Empty => db::open_connection(tmp_db),
    }
}

fn validate_backup(tmp_db: &Path, expected_schema: i64, passphrase: Option<&str>) -> Result<i64> {
    let conn = open_backup_db_conn(tmp_db, passphrase)?;
    db::check_integrity(&conn)?;
    let backup_schema = schema_version(&conn)?;
    if backup_schema > expected_schema {
        return Err(AppError::codedp(
            "backup.schema-newer",
            format!(
                "备份来自更高版本的应用（备份 schema v{backup_schema} > 当前 v{expected_schema}），请升级应用后再恢复"
            ),
            &[&backup_schema.to_string(), &expected_schema.to_string()],
        ));
    }
    if backup_schema < expected_schema {
        tracing::info!(
            backup_schema,
            expected_schema,
            "备份 schema 较旧，恢复后由原位重引导的建连迁移升级"
        );
    }
    Ok(backup_schema)
}

fn validate_backup_sync_component(tmp_sync: &Path, passphrase: Option<&str>) -> Result<()> {
    let conn = open_backup_db_conn(tmp_sync, passphrase)?;
    db::check_integrity(&conn)
}

fn backup_paired_flag(backup_path: &Path) -> bool {
    File::open(backup_path)
        .ok()
        .and_then(|file| zip::ZipArchive::new(file).ok())
        .and_then(|mut archive| read_meta_from_archive(&mut archive).ok())
        .map(|meta| meta.paired)
        .unwrap_or(false)
}

fn extract_db_files(backup_path: &Path, out_main: &Path, out_sync: &Path) -> Result<bool> {
    let file = File::open(backup_path)?;
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => {
            std::fs::copy(backup_path, out_main)?;
            return Ok(false);
        }
    };

    let mut found_main = false;
    let mut found_sync = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name()?.into_owned();
        if name == ZIP_DB_ENTRY {
            let mut out_file = File::create(out_main)?;
            std::io::copy(&mut entry, &mut out_file)?;
            found_main = true;
        } else if name == ZIP_SYNC_ENTRY {
            let mut out_file = File::create(out_sync)?;
            std::io::copy(&mut entry, &mut out_file)?;
            found_sync = true;
        }
    }
    if !found_main {
        return Err(AppError::coded(
            "backup.db-entry-missing",
            format!("备份包内未找到 {}，不是有效的开源记账备份", ZIP_DB_ENTRY),
        ));
    }
    Ok(found_sync)
}
