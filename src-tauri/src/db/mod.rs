use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::error::{AppError, Result};

pub mod balance;
pub mod data_location;
pub mod perf_trace;
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
            M::up(include_str!("../../migrations/V005__search_index.sql")),
            M::up(include_str!(
                "../../migrations/V006__transaction_amount_index.sql"
            )),
            M::up(include_str!(
                "../../migrations/V007__transaction_idempotency_key.sql"
            )),
            M::up(include_str!("../../migrations/V008__app_settings.sql")),
            M::up(include_str!("../../migrations/V009__items.sql")),
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
    iso_at(chrono::Utc::now())
}

/// 把注入的时刻格式化为与 [`now_iso`] 同格式的 UTC ISO 字符串。
/// 供需注入时钟的调用方（如自动备份锚点）使用，保证全仓唯一格式定义。
pub fn iso_at(now: chrono::DateTime<chrono::Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 生成新的 UUID v7（时间有序，适合主键与同步）。
pub fn new_uuid() -> String {
    uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)).to_string()
}

/// 当前设备标识。MVP 阶段使用固定占位值，后续可改为从配置文件读取。
pub fn device_id() -> String {
    String::from("device-1")
}

/// 在指定目录打开库并完成迁移与搜索索引对账（DataLocation 引导之后的建连步骤）。
/// 启动期唯一入口：先经 [`data_location::boot`] 解析库所在目录，再调本函数建连
/// （见 `lib.rs::init_database`）；不要自行拼接库路径。
pub fn open_db_in(db_dir: &Path) -> Result<DbState> {
    let db_path = db_dir.join(data_location::DB_FILE_NAME);
    tracing::info!(db_path = %db_path.display(), "打开数据库");
    let mut conn = open_connection(db_path)?;
    init_db(&mut conn)?;
    // 启动对账：FTS 文档数 ≠ 未删除交易数 → 全量重建（覆盖 V005 迁移前的存量数据）；
    // 一致则消费重建队列（账户/分类改名、绕过应用层的写入产生的待办）。
    crate::commands::search::reconcile_search_index(&conn)?;
    Ok(DbState {
        conn: Arc::new(Mutex::new(conn)),
    })
}

/// 启动失败重置兜底：把当前库改名 `.bak` 保留后重新打开（新建空库）。
/// 只作用于引导解析出的生效目录，绝不删除任何文件。
pub fn reset_db_in(db_dir: &Path) -> Result<DbState> {
    let db_path = db_dir.join(data_location::DB_FILE_NAME);
    let bak_path = db_path.with_extension("db.bak");
    std::fs::rename(&db_path, &bak_path).ok();
    tracing::info!(bak = %bak_path.display(), "已备份原数据库并重置");
    open_db_in(db_dir)
}

/// 校验数据库文件完整性（`PRAGMA integrity_check` 应返回 `ok`）。
pub fn check_integrity(conn: &Connection) -> Result<()> {
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(AppError::from)?;
    if result != "ok" {
        return Err(AppError::Invalid(format!("数据库完整性检查失败: {result}")));
    }
    Ok(())
}

/// 打开数据库连接并启用外键约束（SQLite 默认关闭，需每次连接显式开启）。
/// 所有数据库连接都应通过此函数或其派生函数创建，以保证外键生效。
/// 同时注册耗时 hook（`perf_trace`），覆盖所有 SQL 执行上下文。
pub fn open_connection<P: AsRef<Path>>(path: P) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    perf_trace::install_perf_trace(&conn, perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD);
    Ok(conn)
}

/// 打开内存数据库连接并启用外键约束（用于测试和 BDD 集成测试）。
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    perf_trace::install_perf_trace(&conn, perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD);
    Ok(conn)
}

// ---------------------------------------------------------------------------
// 应用状态
// ---------------------------------------------------------------------------

pub struct DbState {
    pub conn: Arc<Mutex<Connection>>,
}

#[cfg(test)]
mod tests;
