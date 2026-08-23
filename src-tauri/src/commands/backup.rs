use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::db::{self, DbState};
use crate::error::{AppError, Result};

/// zip 包内数据库条目名。
const ZIP_DB_ENTRY: &str = "ledger.db";
/// zip 包内元数据条目名。
const ZIP_META_ENTRY: &str = "backup.json";

/// 备份元数据，写入 zip 包内 `backup.json`。
#[derive(Debug, Serialize, Deserialize)]
struct BackupMeta {
    created_at: String,
    app_version: String,
    schema_version: i64,
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

/// 当前应用期望的 schema 版本（全部迁移执行后的 `user_version`）。
pub fn expected_schema_version() -> Result<i64> {
    let mut conn = db::open_in_memory()?;
    db::init_db(&mut conn)?;
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map_err(AppError::from)
}

/// 读取连接的 schema 版本（`user_version`）。
fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map_err(AppError::from)
}

/// 校验数据库文件完整性。
fn check_integrity(conn: &Connection) -> Result<()> {
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(AppError::from)?;
    if result != "ok" {
        return Err(AppError::Invalid(format!("数据库完整性检查失败: {result}")));
    }
    Ok(())
}

/// 生成与 `path` 同目录的临时文件路径（名称带唯一后缀）。
fn temp_sibling(path: &Path, tag: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup".into());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(
        ".{file_name}.{tag}-{}-{}",
        std::process::id(),
        db::new_uuid()
    ))
}

/// 原子替换：优先 rename（Unix 上覆盖已存在文件），失败时先删除再 rename（Windows 兼容）。
fn replace_file(src: &Path, dst: &Path) -> Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(first) => match std::fs::remove_file(dst).and_then(|_| std::fs::rename(src, dst)) {
            Ok(()) => Ok(()),
            Err(second) => {
                tracing::error!(first = %first, second = %second, "替换文件失败");
                Err(AppError::Io(format!("替换文件失败: {first}（{second}）")))
            }
        },
    }
}

fn cleanup(path: &Path) {
    if path.exists() {
        std::fs::remove_file(path).ok();
    }
}

/// 将当前数据库备份为 zip 包（`ledger.db` + `backup.json`）写入 `target`。
///
/// 通过 `VACUUM INTO` 生成一致的库文件快照，不影响正在进行的写入；打包完成后原子替换目标文件。
pub fn backup_db_to(conn: &Connection, target: &Path, app_version: &str) -> Result<BackupResult> {
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if !parent.is_dir() {
        return Err(AppError::Invalid(format!(
            "备份目标目录不存在: {}",
            parent.display()
        )));
    }

    let tmp_db = temp_sibling(target, "db");
    let tmp_zip = temp_sibling(target, "zip");

    // 1. VACUUM INTO 生成一致的临时库文件（要求目标不存在，故用唯一临时名）。
    conn.execute(
        "VACUUM INTO ?1",
        rusqlite::params![tmp_db.to_string_lossy()],
    )?;

    // 2. 打包 zip：数据库 + 元数据。
    let meta = BackupMeta {
        created_at: db::now_iso(),
        app_version: app_version.to_string(),
        schema_version: schema_version(conn)?,
    };
    let zip_result = (|| -> Result<()> {
        let file = File::create(&tmp_zip)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(ZIP_DB_ENTRY, options)?;
        let mut db_file = File::open(&tmp_db)?;
        std::io::copy(&mut db_file, &mut zip)?;
        zip.start_file(ZIP_META_ENTRY, options)?;
        zip.write_all(serde_json::to_string_pretty(&meta)?.as_bytes())?;
        zip.finish()?;
        Ok(())
    })();
    cleanup(&tmp_db);
    zip_result?;

    // 3. 原子替换目标。
    replace_file(&tmp_zip, target)?;
    cleanup(&tmp_zip);

    let size_bytes = std::fs::metadata(target)?.len();
    tracing::info!(target = %target.display(), size = %size_bytes, schema = %meta.schema_version, "备份完成");
    Ok(BackupResult {
        path: target.to_string_lossy().into_owned(),
        size_bytes,
        schema_version: meta.schema_version,
        created_at: meta.created_at,
    })
}

/// 从备份恢复：提取数据库 → 完整性 + 版本校验（必要时迁移升级）→ 安全备份当前库 → 替换。
///
/// `backup_path` 支持 zip 包（标准格式）与裸 `.db` 文件两种输入。
/// `safety_dir` 用于存放恢复前自动创建的 RestoreSafetyBackup。
pub fn restore_db_from(
    backup_path: &Path,
    db_path: &Path,
    safety_dir: &Path,
    expected_schema: i64,
) -> Result<RestoreResult> {
    let tmp_db = temp_sibling(db_path, "restore");

    // 1. 提取数据库文件（zip 或裸 db）。
    if let Err(e) = extract_db_file(backup_path, &tmp_db) {
        cleanup(&tmp_db);
        return Err(e);
    }

    // 2. 完整性 + 版本校验；备份旧于当前则迁移升级。
    let backup_schema = match validate_backup(&tmp_db, expected_schema) {
        Ok(v) => v,
        Err(e) => {
            cleanup(&tmp_db);
            return Err(e);
        }
    };

    // 3. 安全备份当前库（恢复出错时可回滚）。
    if db_path.exists() {
        std::fs::create_dir_all(safety_dir)?;
        let stamp = db::now_iso().replace([':', 'T'], "-");
        let safety = safety_dir.join(format!("restore-safety-{stamp}.db"));
        std::fs::copy(db_path, &safety)?;
        tracing::info!(safety = %safety.display(), "恢复前已自动备份当前数据库");
    }

    // 4. 替换原库。
    let replace_result = replace_file(&tmp_db, db_path);
    cleanup(&tmp_db);
    replace_result?;

    let restored_at = db::now_iso();
    tracing::info!(schema = %backup_schema, "恢复完成");
    Ok(RestoreResult {
        schema_version: backup_schema,
        restored_at,
    })
}

/// 校验备份数据库文件：完整性检查 + schema 版本策略（旧→新允许并迁移，新→旧拒绝）。
fn validate_backup(tmp_db: &Path, expected_schema: i64) -> Result<i64> {
    let mut conn = db::open_connection(tmp_db)?;
    check_integrity(&conn)?;
    let backup_schema = schema_version(&conn)?;
    if backup_schema > expected_schema {
        return Err(AppError::Invalid(format!(
            "备份来自更高版本的应用（备份 schema v{backup_schema} > 当前 v{expected_schema}），请升级应用后再恢复"
        )));
    }
    if backup_schema < expected_schema {
        tracing::info!(
            backup_schema,
            expected_schema,
            "备份 schema 较旧，恢复时自动迁移升级"
        );
        db::init_db(&mut conn)?;
    }
    Ok(backup_schema)
}

/// 从备份输入提取数据库文件到 `out`：zip 包解出 `ledger.db`，裸 `.db` 直接拷贝。
fn extract_db_file(backup_path: &Path, out: &Path) -> Result<()> {
    let file = File::open(backup_path)?;
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => {
            // 非 zip：按裸 db 处理。
            std::fs::copy(backup_path, out)?;
            return Ok(());
        }
    };

    let mut found = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name == ZIP_DB_ENTRY {
            let mut out_file = File::create(out)?;
            std::io::copy(&mut entry, &mut out_file)?;
            found = true;
        }
    }
    if !found {
        return Err(AppError::Invalid(format!(
            "备份包内未找到 {}，不是有效的 Ledger 备份",
            ZIP_DB_ENTRY
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri 命令
// ---------------------------------------------------------------------------

/// 把当前数据库备份为 zip 包写入 `target_path`（完整文件路径，含文件名）。
#[tauri::command]
pub fn create_backup(app: AppHandle, target_path: String) -> Result<BackupResult> {
    let state = app.state::<DbState>();
    let conn = state.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let app_version = app.package_info().version.to_string();
    backup_db_to(&conn, Path::new(&target_path), &app_version)
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
    let expected = expected_schema_version()?;
    // 恢复期间持有主连接锁，阻塞 IPC 与本地 HTTP API 的并发写。
    let _guard = state.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    restore_db_from(Path::new(&backup_path), &db_path, &dir, expected)
}

/// 重启应用（恢复成功后调用，使新数据以全新状态加载）。
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{init_db, open_connection, open_in_memory};
    use rusqlite::params;

    fn temp_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ledger-backup-test-{tag}-{}-{}.db",
            std::process::id(),
            db::new_uuid()
        ))
    }

    /// 建内存库并写入一条账户 + 一条交易。
    fn seed(conn: &Connection) {
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('acc-1','现金','cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('txn-1','expense',1500,'CNY',1500,'acc-1','2026-02-01','2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test',0)",
            [],
        )
        .unwrap();
    }

    fn count_transactions(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap()
    }

    /// 为每个测试准备独立的安全备份目录（互不干扰）。
    fn temp_safety_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ledger-backup-test-safety-{}-{}",
            std::process::id(),
            db::new_uuid()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn backup_creates_zip_with_db_and_meta() {
        let mut conn = open_in_memory().unwrap();
        init_db(&mut conn).unwrap();
        seed(&conn);

        let target = temp_file("zip");
        let result = backup_db_to(&conn, &target, "0.2.0").unwrap();

        assert!(target.exists());
        assert!(result.size_bytes > 0);
        assert!(result.schema_version >= 4);

        // 校验 zip 内容：两个条目。
        let file = File::open(&target).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 2);
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"ledger.db".to_string()));
        assert!(names.contains(&"backup.json".to_string()));

        // 解出 db 可打开且数据完整。
        let extracted = temp_file("extracted");
        let mut db_entry = archive.by_name("ledger.db").unwrap();
        let mut out = File::create(&extracted).unwrap();
        std::io::copy(&mut db_entry, &mut out).unwrap();
        drop(out);
        let db_conn = open_connection(&extracted).unwrap();
        assert_eq!(count_transactions(&db_conn), 1);
        cleanup(&target);
        cleanup(&extracted);
    }

    #[test]
    fn restore_roundtrip_preserves_data() {
        let mut conn = open_in_memory().unwrap();
        init_db(&mut conn).unwrap();
        seed(&conn);

        let backup = temp_file("rt-backup");
        backup_db_to(&conn, &backup, "0.2.0").unwrap();

        // 目标库先建好，含一条多余交易；恢复后应只剩备份里的数据。
        let db_path = temp_file("rt-db");
        {
            let mut c = open_connection(&db_path).unwrap();
            init_db(&mut c).unwrap();
            seed(&c);
            c.execute(
                "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
                 VALUES ('txn-2','expense',999,'CNY',999,'acc-1','2026-03-01','2026-03-01T00:00:00Z','2026-03-01T00:00:00Z',1,'test',0)",
                [],
            )
            .unwrap();
            assert_eq!(count_transactions(&c), 2);
        }

        let safety_dir = temp_safety_dir();
        let expected = expected_schema_version().unwrap();
        let result = restore_db_from(&backup, &db_path, &safety_dir, expected).unwrap();
        assert_eq!(result.schema_version, expected);

        // 恢复后数据与备份一致（1 条交易）。
        let c = open_connection(&db_path).unwrap();
        assert_eq!(count_transactions(&c), 1);
        // 恢复前的库被安全备份。
        let safeties: Vec<_> = std::fs::read_dir(&safety_dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("restore-safety-")
            })
            .collect();
        assert_eq!(safeties.len(), 1);

        cleanup(&backup);
        cleanup(&db_path);
        std::fs::remove_dir_all(&safety_dir).ok();
    }

    #[test]
    fn restore_rejects_newer_schema() {
        // 构造一个 schema 版本更高的库文件。
        let newer = temp_file("newer");
        {
            let c = open_connection(&newer).unwrap();
            c.execute_batch("PRAGMA user_version = 999").unwrap();
        }
        let db_path = temp_file("db");
        let expected = expected_schema_version().unwrap();
        let tmp_dir = std::env::temp_dir();
        let err = restore_db_from(&newer, &db_path, &tmp_dir, expected)
            .unwrap_err()
            .to_string();
        assert!(err.contains("更高版本"), "错误信息: {err}");
        assert!(!db_path.exists(), "恢复应被拒绝，不产生目标库");
        cleanup(&newer);
        cleanup(&db_path);
    }

    #[test]
    fn restore_supports_bare_db() {
        let mut conn = open_in_memory().unwrap();
        init_db(&mut conn).unwrap();
        seed(&conn);

        // 直接 VACUUM INTO 生成裸 db 文件作为"备份"。
        let bare = temp_file("bare");
        conn.execute("VACUUM INTO ?1", params![bare.to_string_lossy()])
            .unwrap();

        let db_path = temp_file("db2");
        let safety_dir = temp_safety_dir();
        let expected = expected_schema_version().unwrap();
        restore_db_from(&bare, &db_path, &safety_dir, expected).unwrap();
        let c = open_connection(&db_path).unwrap();
        assert_eq!(count_transactions(&c), 1);
        cleanup(&bare);
        cleanup(&db_path);
        std::fs::remove_dir_all(&safety_dir).ok();
    }

    #[test]
    fn backup_fails_when_target_dir_missing() {
        let conn = open_in_memory().unwrap();
        let missing = std::env::temp_dir().join(format!(
            "no-such-dir-{}-{}",
            std::process::id(),
            db::new_uuid()
        ));
        let target = missing.join("x.zip");
        let err = backup_db_to(&conn, &target, "0.2.0")
            .unwrap_err()
            .to_string();
        assert!(err.contains("备份目标目录不存在"));
    }
}
