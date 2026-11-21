use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use super::migrate::init_db;
use super::perf_trace;
use super::runtime::DbState;
use crate::error::{AppError, Result};

pub const DB_FILE_NAME: &str = "ledger.db";

pub fn is_main_db_path(path: &Path) -> bool {
    path.file_name() == Some(std::ffi::OsStr::new(DB_FILE_NAME))
}

pub const SYNC_DB_FILE_NAME: &str = "sync.db";

pub const SYNC_MOUNT_FAILED: &str = "db.sync-mount-failed";

pub const SYNC_SPLIT_USER_VERSION: i64 = 35;

pub fn sync_db_path(main_db_path: &Path) -> std::path::PathBuf {
    main_db_path.with_file_name(SYNC_DB_FILE_NAME)
}

pub const SYNC_TABLES: [&str; 4] = [
    "sync_device",
    "sync_ops",
    "sync_parked_ops",
    "sync_stream_positions",
];
pub(crate) fn sync_alias_attached(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT count(*) FROM pragma_database_list WHERE name = 'sync'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

pub fn snapshot_sync_tables_into(conn: &Connection, path: &Path) -> Result<()> {
    conn.execute(
        "VACUUM sync INTO ?1",
        rusqlite::params![path.to_string_lossy()],
    )?;
    Ok(())
}

pub const CONCURRENT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub fn open_connection_in(db_dir: &Path) -> Result<Connection> {
    let db_path = db_dir.join(DB_FILE_NAME);
    tracing::info!(db_path = %db_path.display(), "打开数据库");
    let mut conn = open_connection(db_path)?;
    init_db(&mut conn)?;
    Ok(conn)
}

pub fn open_db_in(db_dir: &Path) -> Result<DbState> {
    let conn = open_connection_in(db_dir)?;
    let read_conn = open_connection_readonly_in(db_dir)?;
    Ok(DbState::from_pair(conn, read_conn))
}

pub fn reset_db_in(db_dir: &Path) -> Result<DbState> {
    let conn = reset_db_file(db_dir)?;
    let read_conn = open_connection_readonly_in(db_dir)?;
    Ok(DbState::from_pair(conn, read_conn))
}

pub fn reset_db_file(db_dir: &Path) -> Result<Connection> {
    let db_path = db_dir.join(DB_FILE_NAME);
    let sync_path = sync_db_path(&db_path);
    if sync_path.exists() {
        std::fs::rename(&sync_path, sync_path.with_extension("db.bak"))?;
    }
    let bak_path = db_path.with_extension("db.bak");
    std::fs::rename(&db_path, &bak_path).ok();
    tracing::info!(bak = %bak_path.display(), "已备份原数据库并重置");
    let mut conn = open_connection(&db_path)?;
    init_db(&mut conn)?;
    check_integrity(&conn)?;
    Ok(conn)
}

pub fn check_integrity(conn: &Connection) -> Result<()> {
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(AppError::from)?;
    if result != "ok" {
        return Err(AppError::codedp(
            "db.integrity-check-failed",
            format!("数据库完整性检查失败: {result}"),
            &[result.as_str()],
        ));
    }
    Ok(())
}

pub fn open_connection<P: AsRef<Path>>(path: P) -> Result<Connection> {
    let path = path.as_ref();
    finish_open(Connection::open(path)?, None, Some(path))
}

pub fn open_connection_with_passphrase<P: AsRef<Path>>(
    path: P,
    passphrase: &str,
) -> Result<Connection> {
    let path = path.as_ref();
    finish_open(Connection::open(path)?, Some(passphrase), Some(path))
}

pub fn open_connection_unmounted<P: AsRef<Path>>(path: P) -> Result<Connection> {
    let path = path.as_ref();
    finish_open(Connection::open(path)?, None, None)
}

pub fn open_connection_with_passphrase_unmounted<P: AsRef<Path>>(
    path: P,
    passphrase: &str,
) -> Result<Connection> {
    let path = path.as_ref();
    finish_open(Connection::open(path)?, Some(passphrase), None)
}

pub fn open_connection_readonly<P: AsRef<Path>>(path: P) -> Result<Connection> {
    let path = path.as_ref();
    finish_open_readonly(
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?,
        None,
        Some(path),
    )
}

pub fn open_connection_readonly_with_passphrase<P: AsRef<Path>>(
    path: P,
    passphrase: &str,
) -> Result<Connection> {
    let path = path.as_ref();
    finish_open_readonly(
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?,
        Some(passphrase),
        Some(path),
    )
}

pub fn open_connection_readonly_in(db_dir: &Path) -> Result<Connection> {
    open_connection_readonly(db_dir.join(DB_FILE_NAME))
}

pub(crate) fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn attach_sync_db(
    conn: &Connection,
    main_db_path: &Path,
    passphrase: Option<&str>,
    readonly_side: bool,
) -> Result<()> {
    if !is_main_db_path(main_db_path) {
        return Ok(());
    }
    let sync_path = sync_db_path(main_db_path);
    if !sync_path.exists() {
        if readonly_side {
            return Ok(());
        }
        let main_version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| mount_failed_error(&e, passphrase))?;
        if main_version >= SYNC_SPLIT_USER_VERSION {
            let reason = "sync.db 文件缺失";
            return Err(AppError::codedp(
                SYNC_MOUNT_FAILED,
                format!("同步元数据库挂载失败: {reason}"),
                &[reason],
            ));
        }
    }
    let key_clause = passphrase
        .map(|pass| format!(" KEY {}", sql_string_literal(pass)))
        .unwrap_or_default();
    let attach_sql = format!(
        "ATTACH DATABASE {} AS sync{key_clause}",
        sql_string_literal(&sync_path.to_string_lossy()),
    );
    if let Err(e) = conn.execute_batch(&attach_sql) {
        return Err(mount_failed_error(&e, passphrase));
    }
    if let Err(e) =
        conn.query_row::<i64, _, _>("SELECT count(*) FROM sync.sqlite_master", [], |r| r.get(0))
    {
        return Err(mount_failed_error(&e, passphrase));
    }
    tracing::debug!(sync_db = %sync_path.display(), "同步元数据库已挂载");
    Ok(())
}

pub(crate) fn passphrase_incorrect_error() -> AppError {
    AppError::coded(
        "encryption.passphrase-incorrect",
        "口令错误或文件损坏，请重试",
    )
}

pub(crate) fn is_not_a_database_error(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if f.extended_code == rusqlite::ffi::SQLITE_NOTADB
    )
}

fn mount_failed_error(e: &rusqlite::Error, passphrase: Option<&str>) -> AppError {
    if passphrase.is_some() && is_not_a_database_error(e) {
        return passphrase_incorrect_error();
    }
    let reason = match e {
        rusqlite::Error::SqliteFailure(f, _) => f.to_string(),
        other => other.to_string(),
    };
    AppError::codedp(
        SYNC_MOUNT_FAILED,
        format!("同步元数据库挂载失败: {reason}"),
        &[reason.as_str()],
    )
}

fn finish_open_readonly(
    conn: Connection,
    passphrase: Option<&str>,
    main_db_path: Option<&Path>,
) -> Result<Connection> {
    if let Some(passphrase) = passphrase {
        conn.pragma_update(None, "key", passphrase)?;
    }
    conn.busy_timeout(CONCURRENT_BUSY_TIMEOUT)?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    if let Some(path) = main_db_path {
        attach_sync_db(&conn, path, passphrase, true)?;
    }
    perf_trace::install_perf_trace(&conn, perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD);
    Ok(conn)
}

fn finish_open(
    conn: Connection,
    passphrase: Option<&str>,
    main_db_path: Option<&Path>,
) -> Result<Connection> {
    if let Some(passphrase) = passphrase {
        conn.pragma_update(None, "key", passphrase)?;
    }
    conn.busy_timeout(CONCURRENT_BUSY_TIMEOUT)?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    if let Some(path) = main_db_path {
        attach_sync_db(&conn, path, passphrase, false)?;
    }
    perf_trace::install_perf_trace(&conn, perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD);
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = finish_open(Connection::open_in_memory()?, None, None)?;
    conn.execute_batch("ATTACH DATABASE ':memory:' AS sync")?;
    Ok(conn)
}
pub fn open_in_memory_initialized() -> Result<Connection> {
    use std::sync::OnceLock;

    static TEMPLATE: OnceLock<std::result::Result<TemplateBlob, String>> = OnceLock::new();
    struct TemplateBlob {
        main: Vec<u8>,
        sync: Vec<u8>,
    }
    fn derive_template() -> std::result::Result<TemplateBlob, String> {
        let mut conn = open_in_memory().map_err(|e| e.to_string())?;
        init_db(&mut conn).map_err(|e| e.to_string())?;
        let main = conn.serialize("main").map_err(|e| e.to_string())?;
        let sync = conn.serialize("sync").map_err(|e| e.to_string())?;
        Ok(TemplateBlob {
            main: main.to_vec(),
            sync: sync.to_vec(),
        })
    }
    let template = TEMPLATE.get_or_init(derive_template);
    let Ok(template) = template.as_ref() else {
        let reason = match template {
            Err(reason) => reason.clone(),
            Ok(_) => String::new(),
        };
        return Err(AppError::coded(
            "db.template-init-failed",
            format!("内存模板库构建失败，无法还原测试库：{reason}"),
        ));
    };
    let mut conn = open_in_memory()?;
    conn.deserialize_read_exact("main", template.main.as_slice(), template.main.len(), false)?;
    conn.deserialize_read_exact("sync", template.sync.as_slice(), template.sync.len(), false)?;
    Ok(conn)
}
