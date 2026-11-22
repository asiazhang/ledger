use std::sync::OnceLock;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations, SchemaVersion};

use super::schema_guard;
use crate::error::{AppError, Result};

pub const BOOT_SCHEMA_AHEAD: &str = "boot.schema-ahead";

include!(concat!(env!("OUT_DIR"), "/migration_chain.rs"));

pub fn migrations() -> &'static Migrations<'static> {
    static MIGRATIONS: OnceLock<Migrations<'static>> = OnceLock::new();
    MIGRATIONS
        .get_or_init(|| Migrations::new(MIGRATION_CHAIN.iter().map(|sql| M::up(sql)).collect()))
}

pub fn guard_schema_ahead(conn: &Connection) -> Result<()> {
    if let SchemaVersion::Outside(v) = migrations().current_version(conn)? {
        tracing::warn!(
            db_version = v,
            "库 schema 版本超前于应用迁移链（账本曾由更新版本的应用打开），拒绝迁移"
        );
        return Err(AppError::coded(
            BOOT_SCHEMA_AHEAD,
            "账本由更新版本的应用创建，请升级应用后重试",
        ));
    }
    Ok(())
}

pub fn init_db(conn: &mut Connection) -> Result<()> {
    tracing::info!("开始执行数据库迁移");
    guard_schema_ahead(conn)?;
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(AppError::from)?;
    migrations().to_latest(conn)?;
    tracing::info!("数据库迁移完成");
    schema_guard::verify_schema(conn)?;
    conn.execute("PRAGMA foreign_keys = ON", [])
        .map_err(AppError::from)?;
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(AppError::from)
}
