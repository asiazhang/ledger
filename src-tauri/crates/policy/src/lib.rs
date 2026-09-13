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

//! 保单领域 crate（Policy，issue #360 / spec #358 / ADR-0051；spec #1086 /
//! issue #1100 自根包域目录拆出，P3 叶子业务域 crate）：保单静态档案的创建、
//! 编辑、软删除、列表与保单视角统计（实时推导不落库）——写入口的校验与归一化、
//! 口径接线与失效信号回调注入。
//!
//! 接缝：
//! - 域 API（单一权威）：写路径（创建/编辑/删除）与读路径
//!   （列表/保单视角统计），域语言短名；失效信号以 `notify` 回调注入
//!   （回调注入式，仿行情同步域 `sync` 的 emit 注入先例）。
//! - 交易×保单接缝（spec #1086 / issue #1092）：来源列①保单直挂反查经
//!   `ledger-transaction::seams::source` 注册点消费，实现由本 crate
//!   [`install_source_hook`] 装入（壳层启动接线，ADR-0112 决策 5「下层定义
//!   注册点、上层注册实现、壳层启动时接线」）；未注册即码化错误（失败可见）。
//!
//! 依赖方向（spec #1086 / issue #1100 AC）：本 crate 消费基础设施、同步协议与
//! 核心交易域（`ledger-infra` / `ledger-sync-protocol` / `ledger-transaction`）
//! ——统计读路径消费交易域 kind→度量矩阵与本位币折算口径，对根包（壳层）与
//! 任何同级业务域零依赖，反向引用由 cargo 依赖图编译期拒绝（生产依赖面无根包，
//! dev-dependency 环只覆盖测试目标；机器面负向核对住结构守门的 crate 依赖方向，
//! Cargo.toml 注释留痕）。
//!
//! 保司字典（Insurer，issue #712 / ADR-0082）归保险域自有：单消费方 Policy，
//! 不进参考数据域与核心交易域；模型与 CRUD 收口 `insurer`（体量小不拆）。
//! 保单实体、入参与统计行集中 `model`（#420 随域归位），消费方经域路径逐类型
//! 显式 import。
//!
//! **测试实例纪律（dev-dependency 环双实例，ledger-transaction/#1092 同款）**：
//! `cargo test -p ledger-policy` 的依赖图内存在本 crate 的两份实例——被测本
//! 实例与根包图内实例（静态与类型身份分离）。本域行为路径不读交易域接缝注册
//! 静态（接缝方向是交易 → 保单，本 crate 是实现注册侧；测试建库经根包测试
//! 工厂 `tauri_app_lib::test_support::open` 同点接线后，注册静态与生产同形），
//! 域单测直接驱动本实例；经接缝与壳层的旅程由根包侧三层测试（API/命令集成、
//! e2e BDD）走根包图实例覆盖，断言与场景文本零改动。

mod command;
mod crud;
mod insurer;
mod model;
mod stats;
mod validation;

pub use command::{InsurerCommand, PolicyCommand, PolicyCommandRow};
/// 同步重放接缝（跨 crate 消费：sync_engine::registry 经根包再导出面分派外来
/// 保单/保司命令，与本地写同协议。#1100 拆 crate 起 `pub(crate)`→`pub`，签名
/// 与语义不变，#1092 replay_command 同款）。
pub use command::{replay_insurer_command, replay_policy_command};

/// 域 API 再导出：调用面用域语言短名（`policy::list_policies` 等），
/// 与 ADR-0056 定格形状一致（先例：`item` / `scheduled_transactions` 入口再导出）。
pub use crud::{
    create_policy, delete_policy, install_source_hook, list_policies, source_display_by_ids,
    update_policy,
};
pub use insurer::{
    Insurer, InsurerInput, InsurerUpdateInput, create_insurer, create_insurer_by_name,
    delete_insurer, find_insurer_by_name, list_insurers, update_insurer,
};
pub use model::{Policy, PolicyInput, PolicySourceDisplay, PolicyStats};
pub use stats::policy_stats;

#[cfg(test)]
mod tests;
