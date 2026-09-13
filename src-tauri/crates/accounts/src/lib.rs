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

//! 账户域 crate（Account，spec #1086 / issue #1093 自根包域目录拆出，P3 叶子
//! 业务域 crate；域目录化先例 #404，ADR-0056）。
//!
//! 账户 CRUD、自然键幂等创建、币种锁定守卫、黑洞账户即建与余额调整交易编排
//! 在本域收口；IPC 参数解包、事务边界、命令注册和失效信号发射留在
//! `commands::accounts` 壳层。域不依赖壳层；净资产/财务自由度聚合经本入口
//! 复用余额口径（issue #142），余额调整经核心交易域创建编排入口落库（issue #310）。
//!
//! - [`balance`]（余额口径权威，ADR-0071 决策 2 自 `db/balance.rs` 整文件迁入）：
//!   实时计算、缓存整体重算刷新与读取、账户余额清单读取一个模块命中；写路径
//!   副作用接缝（余额刷新，issue #1090）的实现注册点也在此——本域把实现装进
//!   核心交易域的注册点，壳层启动时接线；
//! - [`core`]：CRUD / 幂等创建 / 软删除 / 黑洞账户 / 余额调整编排 + 出资账户
//!   视图接缝实现（issue #1092 注册点的本域实现侧）；
//! - [`command`]（同步命令，issue #860）：op 载荷的账户域形态、产出单点
//!   （`record_local`）与重放分派（`replay_command`）；
//! - [`model`]：账户类型枚举、账户实体、入参与账户余额读模型 DTO 集中本模块
//!   （#419 随域归位），消费方经域路径逐类型显式 import。
//!
//! 依赖方向（spec #1086 / ADR-0112）：本域消费基础设施、同步协议与核心交易域
//! （`ledger-infra` / `ledger-sync-protocol` / `ledger-transaction`），对根包
//! （壳层）与同级业务域零依赖——余额口径消费核心交易域 kind→度量矩阵
//! （`accounts → transaction` 单向，ADR-0071 决策 5 修订后方向）；迁移前核心
//! 交易域对本域的写路径余额重算直调已按挂载点反转收敛（#1090/#1092）：本域
//! 提供实现注册进对方注册点，壳层启动时接线。
//!
//! 依赖方向由编译期强制（issue #1093 AC3）：生产依赖面（lib 目标）没有根包
//! `tauri_app_lib` 与任何同级业务域，构造对它们的引用即编译失败；dev-dependency
//! 环只覆盖测试目标，不构成生产环（本 crate Cargo.toml 注释留痕）。机器面负向
//! 核对住结构守门的 crate 依赖方向（`scripts/check-structure.ts` CRATES +
//! `check-structure.test.ts` 负向夹具）。
//!
//! **测试实例纪律（dev-dependency 环双实例）**：`cargo test -p ledger-accounts`
//! 的依赖图内存在本 crate 的两份实例——被测本实例与根包图内实例（tauri-app 经
//! 生产依赖携带，静态与类型身份分离，ledger-backup/ledger-transaction 同款）。
//! 接缝注册静态由测试工厂（`tauri_app_lib::test_support::open`）接在根包图实例
//! 上，故走接缝的行为路径测试经 `tauri_app_lib::accounts::…` 驱动（同一份源码）；
//! 纯函数与守门用例（不读注册静态）直接驱动本实例。带类型签名的钩子无法跨实例
//! 注册（名义类型不等价），这是测试实例划分的硬约束。

mod command;
mod core;
mod model;

/// 余额口径权威（ADR-0071 决策 2 自 `db/balance.rs` 整文件迁入）：实时计算、
/// 缓存整体重算刷新与读取、账户余额清单读取一个模块命中；契约与依赖边
/// 详见 [`balance`] 模块文档。
pub mod balance;

/// 同步重放（跨 crate 消费：sync_engine 按实体标签分派外来命令，ADR-0091）。
pub use command::replay_command;
pub use command::{AccountCommand, AccountCommandRow};
pub use core::{
    adjust_account_balance, audit_balance_cache, create_account, create_account_idempotent,
    delete_account, ensure_black_hole_account, get_account, install_funding_account_hook,
    list_account_balances_for_api, list_account_balances_with_visibility, list_accounts,
    list_accounts_for_api, update_account,
};
pub use model::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountType,
    AccountUpdateInput, BalanceCacheAudit, BalanceCacheDrift,
};

#[cfg(test)]
mod tests;
