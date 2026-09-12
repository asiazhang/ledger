//! 数据库基础设施模块（issue #1127 按职责拆分后，本文件只做声明与再导出）：
//! - [`migrate`]：迁移链与 `init_db`（schema 守卫尾部接线，ADR-0100）；
//! - [`connection`]：建连 / 重置 / 完整性检查 / 内存库；
//! - [`runtime`]：连接层统一写入口与提交点后置钩子（ADR-0032）、阻塞线程池
//!   helper（ADR-0069 形状乙）与 [`DbState`]。
//!
//! 其余子模块（book_registry / boot / data_location / encryption /
//! passphrase_cache / perf_trace / query / schema_guard / tx_scope）各承载
//! 单一主题。既有消费方 `crate::db::…` 路径经再导出零改动。

pub mod book_registry;
pub mod boot;
pub mod connection;
pub mod data_location;
pub mod encryption;
pub mod migrate;
pub mod passphrase_cache;
pub mod perf_trace;
pub mod query;
pub mod runtime;
pub mod schema_guard;
pub mod tx_scope;

pub use connection::{
    check_integrity, open_connection, open_connection_in, open_connection_with_passphrase,
    open_db_in, open_in_memory, reset_db_file, reset_db_in,
};
pub use migrate::{init_db, schema_version};
pub use runtime::{AfterCommitHook, DbState, register_after_commit_hook, run_db, write};

// 迁移集合保持 crate 内可见面（tests 与 schema_guard 经此消费，非公开 API）。
pub(crate) use migrate::migrations;

// ---------------------------------------------------------------------------
// 时间与身份工厂（暂住：issue #1128 将升顶层 ids 模块——非数据库关切）
// ---------------------------------------------------------------------------

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

/// 确定性 UUID v5 的本仓命名空间（跨端一致派生 id 的派生根；先例：V004 默认
/// 种子的确定性 UUID v5——同名恒同值，保证各端独立派生不产生重复行）。
pub const DETERMINISTIC_NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes(*b"ledger_sync_v5_1");

/// 确定性 UUID v5：同名同空间跨端恒同值（同步场景的确定性落地身份）。
pub fn deterministic_uuid(name: &str) -> String {
    uuid::Uuid::new_v5(&DETERMINISTIC_NAMESPACE, name.as_bytes()).to_string()
}

#[cfg(test)]
mod tests;
