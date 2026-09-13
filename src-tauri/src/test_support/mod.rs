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
//!
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
//! **#956 追加**（通道线格式替身）：命令面集成测试与 BDD 步骤层各自手工复制的
//! 「构造合法通道段 + 归并 manifest」（含各自的 `sha256_hex`）→
//! [`publish_raw_segment`]（`test_support::channel`）。共享物是**通道线格式契约**
//! （字节级成帧），不是建库/种子/默认值集，ADR-0086 决策 9 不破；由此新增一条
//! `test_support → sync_engine` 测试专用边（登记处：ADR-0084 迁移状态段），
//! 该边由测试豁免路径消费、不进产品依赖图。
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
pub mod channel;
pub mod s3;
mod seed;
#[cfg(test)]
mod tests;
pub mod webdav;

pub use assert::{
    assert_balance_cache_matches_realtime, extract_check_in_literals, read_scalar_i64,
};
pub use channel::publish_raw_segment;
pub use s3::{S3Addressing, S3Deny, S3ObservedRequest, S3Stub, S3StubConfig, spawn_s3_stub};
pub use seed::{
    seed_account, seed_exchange_rate, seed_fx_rate_history, seed_instrument, seed_investment_setup,
    seed_price_history,
};
pub use webdav::{WebDavStub, spawn_webdav_stub};

use rusqlite::Connection;

/// 夹具簿记戳的统一固定时刻（ADR-0084 决策 5）：种子的 created_at/updated_at 等
/// 非行为输入列由工厂内部发放此值，调用点不再出现默认时刻字面量；域时刻是测试的
/// 行为输入，经种子参数显式传入、默认值引用本常量。
pub const FIXED_NOW: &str = "2026-01-01T00:00:00Z";

/// 零配置打开已初始化的内存测试库：`db::open_in_memory()`（外键 + perf hook）+
/// `db::init_db()`（迁移 + 默认种子）两行序的唯一承载（ADR-0084 决策 3：建库的
/// 全部现状就是内存库 + 迁移，无配置项）。文件库/加密是 BDD 场景，不入本工厂。
pub fn open() -> Connection {
    // 提交点后置动作接线（spec #1086 / issue #1088）：测试库与生产同形——连接层
    // 写入口的副作用实现由域侧提供，建库单点负责注册（幂等）。
    crate::backup::install_after_commit_hook();
    // 写后即时同步接线（#1089）：测试库与生产同形——op 产出单点在协议 crate，
    // 响应闭包（去抖合流）由同步域提供，此处登记（幂等，先装者优先）；调度未
    // 拉起时信号投递仍是零动作。
    crate::sync_engine::trigger::install_after_write_hook();
    // 写路径副作用接缝接线（issue #1090 / #1091）：测试库与生产同形——余额刷新
    // 实现由账户域、计划来源解析实现由定时计划域提供，建库单点负责注册（幂等，
    // 先装者优先）；期次落账置脏（#1090）实现由备份域（#1091 起为 `ledger-backup`
    // crate）提供、定时计划域注册点对装，追补触发（#1091 挂载点④）实现由定时
    // 计划域提供、备份域注册点对装——两个域互相零直接依赖，接线都在本单点。
    crate::accounts::balance::install_balance_refresh_hook();
    crate::scheduled_transactions::install_plan_source_hook();
    crate::scheduled_transactions::auto_run::register_after_occurrence_hook(
        crate::backup::occurrence_dirty_hook,
    );
    crate::backup::register_catch_up_hook(crate::scheduled_transactions::auto_run::catch_up_hook);
    // 交易域接缝接线（issue #1092 / #1180）：测试库与生产同形——六向实现经组合
    // 入口一次装入（幂等，先装者优先）。
    crate::transaction_wiring::install_all();
    let mut conn = crate::db::open_in_memory().expect("打开内存测试库");
    crate::db::init_db(&mut conn).expect("初始化内存测试库");
    conn
}
