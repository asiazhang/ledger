// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

//! 物品（Item）领域 crate（spec #113 / ADR-0014；spec #1086 / issue #1099 自根包
//! 域目录拆出，P3 叶子域）：耐用实物物品的创建、修改、处置、软删除、列表与
//! 「每天使用成本」读取聚合——写入口的归一化与校验、溯源准入、口径接线与失效
//! 信号回调注入。IPC 参数解包、事务边界、命令注册和失效信号发射留在物品命令
//! 壳层（`commands::item`）。
//!
//! 接缝：
//! - [`cost`]（DailyUsageCost 权威）：「每天使用成本」纯计算，分子（总成本 − 残值）
//!   ÷ 分母（购买日 → 目标日的日历天数，含起止两端）。
//! - [`domain`]（域 API，单一权威）：写路径（创建/修改/处置/删除）与读路径
//!   （列表/单件每天成本/在用合计）七项，域语言短名；失效信号以 `notify`
//!   回调注入（回调注入式，仿行情同步域 `sync` 的 emit 注入先例）。
//! - [`guard`]（溯源守卫，ADR-0025 创建唯一入口的准入接缝）：关联购买交易的
//!   解析、校验与自动带出——溯源必填/唯一在创建时刻强制。
//!
//! 交易×物品接缝（spec #1086 / issue #1092）：核心交易域来源列③的物品反查经
//! `ledger-transaction::seams::source` 注册点消费，实现由本 crate
//! [`domain::install_source_hook`] 装入（壳层启动接线，ADR-0112 决策 5「下层定义
//! 注册点、上层注册实现、壳层启动时接线」）；未注册即码化错误（失败可见）。
//!
//! 依赖方向（spec #1086 / issue #1099 AC）：本 crate 消费基础设施、同步协议与
//! 核心交易域（`ledger-infra` / `ledger-sync-protocol` / `ledger-transaction`），
//! 对根包（壳层）与同步域零直接依赖——同步命令契约、op 产出与设备标识走协议
//! crate，重放分派住 sync_engine（业务域→同步域零容忍，ADR-0101 决策 4b）。
//! 依赖方向由编译期强制（issue #1099 AC3）：生产依赖面（lib 目标）没有根包
//! `tauri_app_lib`，构造对它的引用即编译失败；dev-dependency 环只覆盖测试目标，
//! 不构成生产环（本 crate Cargo.toml 注释留痕）。
//!
//! **测试实例纪律（dev-dependency 环双实例，ledger-transaction/#1092 同款）**：
//! `cargo test -p ledger-item` 的依赖图内存在本 crate 的两份实例——被测本实例
//! 与根包图内实例（静态与类型身份分离）。本域行为路径不读交易域接缝注册静态
//! （接缝方向是交易 → 物品，本 crate 是实现注册侧；金额折算消费的
//! `ledger-transaction` 注册静态由测试工厂接在依赖图内唯一交易域实例上），
//! 域单测直接驱动本实例；经接缝与壳层的旅程由根包侧三层测试（API/命令集成、
//! e2e BDD）走根包图实例覆盖，断言与场景文本零改动。测试数据库工厂经
//! dev-dependency 取自根包（`tauri_app_lib::test_support::open`，ADR-0084）。

pub mod command;
pub mod cost;
pub mod domain;
pub mod guard;
mod model;

/// 域 API 再导出：调用面用域语言短名（`item::create_item` 等），
/// 与 ADR-0056 阶段 1 定格形状一致（先例：`scheduled_transactions` 入口再导出）。
pub use command::{ItemCommand, ItemCommandRow};
pub use domain::*;
pub use model::{
    Item, ItemDailyCost, ItemDailyTotal, ItemDisposeInput, ItemInput, ItemSourceDisplay,
    ItemStatus, ItemWithDailyCost,
};

/// 同步重放（跨 crate 消费：sync_engine 分派接缝经根包再导出面执行外来命令，
/// ADR-0091。#1099 拆 crate 起 `pub(crate)`→`pub`，签名与语义不变）。
pub use command::replay_command;

#[cfg(test)]
mod tests;
