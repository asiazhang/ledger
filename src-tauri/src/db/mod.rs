use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use tauri::Manager;

use crate::error::{AppError, Result};

pub mod balance;
pub mod query;

/// 迁移集合。新增 schema 变更或种子数据时，在 `src-tauri/migrations/` 下新建
/// `V00X__名称.sql`，并在 `migrations()` 的 `vec!` 里追加
/// `M::up(include_str!("../migrations/V00X__名称.sql"))`。
/// 版本由 SQLite 的 `user_version` 字段自动追踪，无需手动维护版本表。
fn migrations() -> &'static Migrations<'static> {
    static MIGRATIONS: OnceLock<Migrations<'static>> = OnceLock::new();
    MIGRATIONS.get_or_init(|| {
        Migrations::new(vec![
            M::up(include_str!("../../migrations/V001__initial.sql")),
            M::up(include_str!("../../migrations/V002__investment.sql")),
            M::up(include_str!(
                "../../migrations/V003__scheduled_transactions.sql"
            )),
            M::up(include_str!("../../migrations/V004__seed_defaults.sql")),
            M::up(include_str!(
                "../../migrations/V005__instruments_market.sql"
            )),
            M::up(include_str!("../../migrations/V006__import_dedup.sql")),
            M::up(include_str!(
                "../../migrations/V007__black_hole_accounts.sql"
            )),
        ])
    })
}

/// 初始化数据库 schema 与默认种子数据（全部由迁移驱动）。
pub fn init_db(conn: &mut Connection) -> Result<()> {
    tracing::info!("开始执行数据库迁移");
    migrations().to_latest(conn)?;
    tracing::info!("数据库迁移完成");
    Ok(())
}

/// 当前 UTC 时间 ISO 字符串。
pub fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 生成新的 UUID v7（时间有序，适合主键与同步）。
pub fn new_uuid() -> String {
    uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)).to_string()
}

/// 当前设备标识。MVP 阶段使用固定占位值，后续可改为从配置文件读取。
pub fn device_id() -> String {
    String::from("device-1")
}

/// 打开数据库连接并启用外键约束（SQLite 默认关闭，需每次连接显式开启）。
/// 所有数据库连接都应通过此函数或其派生函数创建，以保证外键生效。
pub fn open_connection<P: AsRef<Path>>(path: P) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    Ok(conn)
}

/// 打开内存数据库连接并启用外键约束（用于测试和 BDD 集成测试）。
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    Ok(conn)
}

// ---------------------------------------------------------------------------
// 应用状态
// ---------------------------------------------------------------------------

pub struct DbState {
    pub conn: Arc<Mutex<Connection>>,
}

pub fn open_db(app: &tauri::AppHandle) -> Result<DbState> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Io(e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    let db_path = dir.join("ledger.db");
    tracing::info!(db_path = %db_path.display(), "打开数据库");
    let mut conn = open_connection(db_path)?;
    init_db(&mut conn)?;
    Ok(DbState {
        conn: Arc::new(Mutex::new(conn)),
    })
}

#[cfg(test)]
mod tests;
