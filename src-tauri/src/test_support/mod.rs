//! 统一测试数据库工厂与共享断言库（spec #728 / issue #751 / ADR-0084）。
//!
//! 建库与种子知识的唯一入口：`open()` 吸收 `db::open_in_memory()` + `db::init_db()`
//! 两行序（外键 + perf hook + 迁移 + 种子）；`seed::*` 吸收跨域重复的实体列清单与
//! 投资铺垫组合；`assert::*` 吸收「缓存回填 == 实时计算」不变式对拍（ADR-0067 的
//! 测试保障面收归唯一维护点）与通用白盒行读取器。以 `&Connection` 为中心的自由
//! 函数集，不引入包装类型与 builder 链（ADR-0084 决策 3）。
//!
//! **准入规则**（ADR-0084 决策 1）：工厂只收跨 ≥2 域重复的夹具；单域特有的种子、
//! 输入构造器与读取器留域薄皮（如 `make_input` 等域语义构造器）。守门豁免清单
//! 即此规则的机器面（守门票 #752）。
//!
//! **可见性**（ADR-0084 决策 2）：`pub mod` + `#[doc(hidden)]`，先例 `test_utils.rs`
//! ——外部集成测试（`tests/api_server/`）链接非 `#[cfg(test)]` 构建的 lib，看不到
//! `#[cfg(test)]` 模块，这是仓库已验证的共享机制；`#[doc(hidden)]` 使其不进入文档。
//!
//! **固定时刻**（ADR-0084 决策 5）：[`FIXED_NOW`] 是夹具簿记戳（created_at/updated_at
//! 等非行为输入列）的统一发放值，种子内部发放、调用点零字面量；域时刻（价格/汇率
//! 序列点、预算窗口、定时触发点等测试行为输入）不吸收——需要时间推进的种子参数
//! 显式收时刻。守门票（#752）禁 `FIXED_NOW` 值的字面量出现在工厂之外。
//!
//! **本版收编清单**（吸收体 → 工厂成员，供按域迁移票 #753–#757 核对）：
//! - 建库两行序（transaction/investment/reports/policy/merchants/item/sync/
//!   scheduled_transactions/db 等域薄皮与 `tests/api_server/common.rs` 逐字重复）→ [`open`]；
//! - `transaction/tests/common.rs` 与 `investment/tests/common.rs` 的 `insert_account`
//!   同体函数 → [`seed_account`]（全位置归一签名，ADR-0084 决策 4）；
//! - `db/tests/common.rs` 与 `investment/tests/common.rs` 的 `insert_instrument` 同体
//!   函数 → [`seed_instrument`]；
//! - `db/tests/common.rs`、`investment/tests/trend.rs` 的 price/fx 周采样插入同体函数
//!   → [`seed_price_history`] / [`seed_fx_rate_history`]；
//! - `investment/tests/common.rs` 的 `insert_rate_1_1` / `insert_rate` 及 transaction、
//!   api_server 各处 `exchange_rates` 同形状插入 → [`seed_exchange_rate`]；
//! - `transaction/tests/common.rs`（及 batch_create.rs 逐字副本）的
//!   `setup_investment_account` 与 `tests/api_server/investment_migration.rs` 的
//!   `seed_investment_account` → [`seed_investment_setup`]；
//! - `transaction/tests/balance_cache.rs` 与 `db/tests/balance_cache.rs` 各自维护的
//!   「回填 == 实时」对拍断言 → [`assert_balance_cache_matches_realtime`]（本票唯一
//!   的既有调用改动：两域旧断言体删除改调共享版本）。
//!
//! 说明：集成测试 `tests/api_server/` 链接的是非 `#[cfg(test)]` 构建的 lib，
//! 因此本模块不能仅以 `#[cfg(test)]` 编译；对生产二进制的影响只是一些未使用的
//! 测试辅助函数（可被编译器消除）。
//!
// C 类豁免（ADR-0060）：仅测试用——本模块被集成测试以非 cfg(test) 构建链接，
// 无法经 crate 根 cfg(test) 豁免覆盖，故文件级放行六件套；生产路径不得消费本模块。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable
)]

mod assert;
mod seed;
#[cfg(test)]
mod tests;

pub use assert::{assert_balance_cache_matches_realtime, read_scalar_i64};
pub use seed::{
    seed_account, seed_exchange_rate, seed_fx_rate_history, seed_instrument, seed_investment_setup,
    seed_price_history,
};

use rusqlite::Connection;

/// 夹具簿记戳的统一固定时刻（ADR-0084 决策 5）：种子的 created_at/updated_at 等
/// 非行为输入列由工厂内部发放此值，调用点不再出现默认时刻字面量；域时刻是测试的
/// 行为输入，经种子参数显式传入、默认值引用本常量。
pub const FIXED_NOW: &str = "2026-01-01T00:00:00Z";

/// 零配置打开已初始化的内存测试库：`db::open_in_memory()`（外键 + perf hook）+
/// `db::init_db()`（迁移 + 默认种子）两行序的唯一承载（ADR-0084 决策 3：建库的
/// 全部现状就是内存库 + 迁移，无配置项）。文件库/加密是 BDD 场景，不入本工厂。
pub fn open() -> Connection {
    let mut conn = crate::db::open_in_memory().expect("打开内存测试库");
    crate::db::init_db(&mut conn).expect("初始化内存测试库");
    conn
}
