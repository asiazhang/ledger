//! 数据库基础设施模块（issue #1127 按职责拆分后，本文件只做声明与再导出；
//! 时间与身份工厂已自 #1128 升顶层 [`crate::ids`]，原路径经再导出保持；
//! #1131 起引导层五模块升顶层 [`crate::boot`]，原路径经再导出保持）：
//! - [`migrate`]：迁移链与 `init_db`（schema 守卫尾部接线，ADR-0100）；
//! - [`connection`]：建连 / 重置 / 完整性检查 / 内存库；
//! - [`runtime`]：连接层统一写入口与提交点后置钩子（ADR-0032）、阻塞线程池
//!   helper（ADR-0069 形状乙）与 [`DbState`]。
//!
//! 其余子模块（perf_trace / query / schema_guard / tx_scope）各承载单一主题。
//! 既有消费方 `crate::db::…` 路径经再导出零改动。

pub mod connection;
pub mod migrate;
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

// 时间与身份工厂现住顶层 [`crate::ids`]（issue #1128 / ADR-0111 决策 2：
// 非数据库关切，文件工具等原语引用不穿透 db）；既有 `crate::db::…` 调用点
// 与协议 crate 的 `ledger_infra::db::…` 路径经本再导出保持零改动。
pub use crate::ids::{DETERMINISTIC_NAMESPACE, deterministic_uuid, iso_at, new_uuid, now_iso};

// 引导层五模块现住顶层 [`crate::boot`]（issue #1131 / ADR-0111 决策 2：
// 开库前的引导关切不是 db 库机制，依赖方向 boot → db 单向）；既有
// `crate::db::{boot, data_location, book_registry, encryption,
// passphrase_cache}::…` 调用点与协议 crate 的 `ledger_infra::db::…` 路径经
// 本再导出保持零改动（#1128 的 ids 同款口径：路径兼容面，非机制依赖）。
pub use crate::boot::{
    book_registry, data_location, disposition as boot, encryption, passphrase_cache,
};

#[cfg(test)]
mod tests;
