//! 迁移链与 schema 初始化（自 `db/mod.rs` 按职责拆出，issue #1127，纯移动）：
//! 迁移集合定义、`init_db` 尾部单点（schema 守卫接线，ADR-0100）与 schema
//! 版本读取（`user_version`，迁移自动追踪的库级事实）。

use std::sync::OnceLock;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations, SchemaVersion};

use super::schema_guard;
use crate::error::{AppError, Result};

/// 启动期「库 schema 版本超前」的稳定错误码（ADR-0050 只增不改）：库
/// `user_version` 高于本应用迁移链顶（账本曾由更新版本的应用打开过）时，旧版
/// 二进制不应也无法继续迁移——rusqlite_migration 会以 `DatabaseTooFarAhead`
/// 拒绝，此处前置判定（[`init_db`]）转本码。不复用 `boot.db-unreadable`：库
/// 文件完好无损，重置会毁掉完好的新库、恢复出的备份同样是超前库，两条恢复
/// 通道都通向死路；前端失败恢复屏按码只呈现升级引导（与漂移码
/// [`crate::db::schema_guard::BOOT_SCHEMA_DRIFT`] 同款「按码区分处置」）。错误模板见
/// `packages/i18n/src/locales/{zh-CN,en-US}/errors.json` 的 `boot.schema-ahead`。
pub const BOOT_SCHEMA_AHEAD: &str = "boot.schema-ahead";

// 迁移链 SQL 清单与链长：`crates/infra/build.rs` 按目录投影生成，勿手改；
// 单一事实源 = `src-tauri/migrations/` 目录本身（issue #2199）。
include!(concat!(env!("OUT_DIR"), "/migration_chain.rs"));

/// 迁移集合。新增 schema 变更或种子数据时，只在 `src-tauri/migrations/` 下新建
/// `V00X__名称.sql`——注册链（[`MIGRATION_CHAIN`]）与链长（[`MIGRATION_COUNT`]）
/// 由 `crates/infra/build.rs` 按目录投影生成，无手工清单（issue #2199；#2197 现场
/// 即漏改手工副本而测试红）。版本由 SQLite 的 `user_version` 字段自动追踪，无需
/// 手动维护版本表。
/// pub 语义边界：生产路径一律经 [`init_db`]（迁移 + 守卫单点）；pub 只为测试侧
/// 构造历史世界（如旧形态备份夹具的拆库前形态，`to_version` 停版）。
pub fn migrations() -> &'static Migrations<'static> {
    static MIGRATIONS: OnceLock<Migrations<'static>> = OnceLock::new();
    MIGRATIONS
        .get_or_init(|| Migrations::new(MIGRATION_CHAIN.iter().map(|sql| M::up(sql)).collect()))
}

/// 库版本超前守卫（[`BOOT_SCHEMA_AHEAD`] 的判定本体，[`init_db`] 首步接线）：
/// 库 `user_version` 高于本应用迁移链顶（账本曾由更新版本的应用打开过）时
/// 拒绝——旧版二进制不应也无法继续迁移（rusqlite_migration 将以
/// `DatabaseTooFarAhead` 拒绝，本守卫先行转码）。库与版本位均保持原样。
/// 单独成函数供域内直测（ADR-0084 守门纪律：测试不直呼 `init_db`）。
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

/// 初始化数据库 schema 与默认种子数据（全部由迁移驱动）。
///
/// 外键时序：迁移批次整体包在 rusqlite_migration 的事务里，`PRAGMA foreign_keys`
/// 在事务内是 no-op——故必须在进入迁移前显式关闭、提交并过守卫后恢复。关闭是
/// SQLite 官方表重建流程（12 步，ALTERNATIVE 规程）的前提：V034 放宽
/// `transactions.account_id` 的 NOT NULL 需 DROP+RENAME 重建该表，外键开着时
/// DROP 的隐式 DELETE 会触发子表 CASCADE（security_transactions 清库级灾难）；
/// 关闭下 DROP 不触发任何外键动作，子行原样保留、RENAME 后引用自动解析。
/// 恢复失败即连接不可用（Err 上抛，不静默带外键缺口运行）。
pub fn init_db(conn: &mut Connection) -> Result<()> {
    tracing::info!("开始执行数据库迁移");
    // 库版本超前判定（先于迁移执行）：前端失败恢复屏按码只呈现升级引导。
    guard_schema_ahead(conn)?;
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(AppError::from)?;
    migrations().to_latest(conn)?;
    tracing::info!("数据库迁移完成");
    // Schema 漂移守卫（issue #971/#992 / ADR-0100）：to_latest 只比 user_version，
    // 「版本已达最新但 schema 内容漂移」的库在此确定性拦截。守卫接线是尾部
    // 单点：全部生产建连路径统一收口本函数，一处接线零遗漏。接线负向判据
    //（#963 惯例 / ADR-0087）：删除本调用，e2e「缺列漂移的明文库启动进入
    // 失败状态」场景变红。
    schema_guard::verify_schema(conn)?;
    conn.execute("PRAGMA foreign_keys = ON", [])
        .map_err(AppError::from)?;
    Ok(())
}

/// 当前 schema 版本（SQLite `user_version`，迁移自动追踪）。
///
/// 同步 op 产生时随身携带（issue #855 / ADR-0091：产生时 schema 版本），
/// schema 偏斜判定（#856）依据；作为通用库级事实收口在基础设施。
pub fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(AppError::from)
}
