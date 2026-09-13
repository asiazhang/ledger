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

//! 预算领域 crate（Budget，ADR-0029 / ADR-0052；spec #1086 / issue #1101 自根包
//! 域目录拆出，P3 叶子业务域 crate）：预算 CRUD、软删除与当前周期进度（实时推导
//! 不落库）均在本 crate 收口；IPC 参数解包、事务边界与命令注册留在
//! `commands::budget.rs` 壳层。
//!
//! 进度口径：预算 spent = `expense_net`（毛支出 − 退款，退款冲减支出），参与
//! kind 由核心交易域 kind→度量矩阵单一真源导出（[`progress::budget_progress_rows`]，
//! 窗口为注入 `today` 所在自然月/年，与存储的 start_date 无关）；本 crate 不定义
//! 口径，只消费 `ledger-transaction::amount`。
//!
//! 依赖方向（spec #1086 / issue #1101 AC）：本 crate 消费基础设施、同步协议与
//! 核心交易域（`ledger-infra` / `ledger-sync-protocol` / `ledger-transaction`），
//! 对根包（壳层）与任何同级业务域零依赖，无接缝无注册点、壳层启动零接线；
//! 反向引用由 cargo 依赖图编译期拒绝（生产依赖面无根包，dev-dependency 环只
//! 覆盖测试目标；机器面负向核对住结构守门的 crate 依赖方向，Cargo.toml 注释
//! 留痕）。
//!
//! **测试实例纪律（dev-dependency 环双实例，ledger-transaction/#1092 同款）**：
//! `cargo test -p ledger-budget` 的依赖图内存在本 crate 的两份实例——被测本
//! 实例与根包图内实例（静态与类型身份分离）。本域行为路径不消费任何域间接缝
//! 注册静态（本 crate 无接缝无注册点），域单测直接驱动本实例；经壳层的旅程由
//! 根包侧三层测试（API/命令集成、e2e BDD）走根包图实例覆盖，断言与场景文本
//! 零改动。

mod command;
pub mod crud;
mod model;
mod progress;

pub use command::BudgetCommand;
/// 同步重放接缝（跨 crate 消费：sync_engine::registry 经根包再导出面分派外来
/// 预算命令，与本地写同协议。#1101 拆 crate 起 `pub(crate)`→`pub`，签名与语义
/// 不变，#1092 replay_command 同款）。
pub use command::replay_command;
pub use crud::{create_budget, delete_budget, list_budgets, update_budget};
pub use model::{Budget, BudgetInput, BudgetPeriod, BudgetProgress, BudgetUpdateInput};
pub use progress::budget_progress_rows;

#[cfg(test)]
mod tests;
