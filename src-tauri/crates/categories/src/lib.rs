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

//! 分类领域 crate（Category，spec #1086 / issue #1094 自根包域目录拆出，P3
//! 叶子域；域目录化先例 #404，ADR-0056）。
//!
//! 分类 CRUD、自然键幂等创建、两级分类校验、预算删除守卫（issue #355）与排序
//! 重排在本域收口；IPC 参数解包、事务边界、命令注册和失效信号发射留在壳层
//! `commands::categories`（根包）。域不依赖壳层。
//!
//! 模块：
//! - [`command`]：同步命令（issue #860 / ADR-0091）——op 载荷的分类域形态
//!   （[`CategoryCommand`] / [`CategoryCommandRow`]）、产出单点（`record_local`）
//!   与重放执行（`replay_command`，sync_engine 重放注册表跨 crate 消费）。
//! - [`core`]：CRUD / 幂等创建 / 软删除 / 两级分类校验 / 预算删除守卫 / 排序重排。
//! - [`model`]：分类实体与入参、排序项集中本模块（#419 随域归位），消费方经
//!   域路径逐类型显式 import（禁止 glob）。
//!
//! 依赖方向（spec #1086 / ADR-0112）：本域消费基础设施与同步协议
//! （`ledger-infra` / `ledger-sync-protocol`），对根包（壳层）与任何业务域
//! 零依赖——本域对交易域亦零依赖（预算删除守卫直查 `budgets` 表，不经交易域），
//! 依赖面是「基础设施、协议、核心交易域」允许集的子集。
//!
//! 依赖方向由编译期强制（issue #1094 AC3）：生产依赖面（lib 目标）没有根包
//! `tauri_app_lib` 与任何业务域，构造对它们的引用即编译失败；dev-dependency
//! 环只覆盖测试目标，不构成生产环（本 crate Cargo.toml 注释留痕）。机器面
//! 负向核对住结构守门的 crate 依赖方向（`scripts/check-structure.ts` CRATES +
//! `check-structure.test.ts` 负向夹具）——本 crate 不能用 `use tauri_app_lib::…`
//! 的 compile_fail 文档用例承载该负向例：dev-dependency 对 doctest 可见，会
//! 击穿该形态（先例说明见 ledger-backup / ledger-transaction crate 根文档）。
//!
//! **测试实例纪律（dev-dependency 环双实例）**：`cargo test -p ledger-categories`
//! 的依赖图内存在本 crate 的两份实例——被测本实例与根包图内实例（tauri-app 经
//! 生产依赖携带，静态与类型身份分离，ledger-transaction 先例同款）。本域无
//! 接缝、无注册点静态（op 产出直呼协议面 `record_local`，协议 crate 在依赖图
//! 内单实例共享），带类型签名的钩子不存在，两份实例行为不可区分——域行为测试
//! 直接驱动本实例、建库经根包测试工厂（`tauri_app_lib::test_support::open`）
//! 是同一份源码的等价路径，无需像 ledger-transaction（接缝静态在根包图实例上
//! 接线）那样强制经 `tauri_app_lib::categories::…` 驱动。依据留痕于本段与
//! Cargo.toml dev-dependencies 注释。

mod command;
mod core;
mod model;

/// 重放执行（跨 crate 消费：sync_engine 重放注册表 CategoryBinding 经根包
/// 再导出面调用，ADR-0101）。拆 crate 时 `pub(crate)` 升 `pub`，签名与语义
/// 不变（#1092 `replay_command` 同款）。
pub use command::replay_command;
pub use command::{CategoryCommand, CategoryCommandRow};
pub use core::{
    create_category, create_category_idempotent, delete_category, list_categories,
    reorder_categories, update_category,
};
pub use model::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};

#[cfg(test)]
mod tests;
