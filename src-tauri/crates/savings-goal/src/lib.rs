// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块）经 crate 根 cfg(test) 整体放行，生产构建
// 零放宽。
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

//! 储蓄目标领域 crate（SavingsGoal，spec #1750 / issue #1751 / #1752 / ADR-0133；
//! 单列小域，先例物品 / 保单 / 实物资产）：创建目标时在同一事务内经账户域公开写
//! 入口自动建其专属账户（`other` 类型、目标经绑定持有账户身份，1 目标 : 1 账户）、
//! 编辑目标（四字段全量替换 + 改名联动专属账户——目标名权威、账户名随动只读），
//! 以及蓄水进度读数（已存 = 专属账户余额、还差、达成态——余额口径、读余额缓存
//! ADR-0067）。语义与边界见储蓄目标域词汇表与 ADR-0133。
//!
//! 接缝：域 API 单一权威——写路径 [`create_savings_goal`]（创建联动本体，目标
//! 金额正数码化守卫先行）与 [`update_savings_goal`]（编辑全量替换 + 改名联动，
//! issue #1752）、读路径 [`list_savings_goal_progress`]（进度 = 余额，
//! `expense`，全部走核心交易域既有写入协议（ADR-0133 决策 1），本 crate 只写
//! 目标本体一行；达成是读时派生的纯展示态，不落库、不反向挂交易写钩子。
//!
//! 依赖方向（spec #1750 / issue #1751 AC）：本 crate 消费基础设施、同步协议与
//! 两个同级业务域（`ledger-infra` / `ledger-sync-protocol` / `ledger-accounts`
//! / `ledger-transaction`，域→域横向依赖 ADR-0056 决策 2 允许），对根包（壳层）
//! 与多端同步域零依赖，无接缝无注册点、壳层启动零接线；同步命令（Goal
//! DomainCommand）随多端同步票接入，设备标识现走 `ledger-sync-protocol`。
//! 反向引用由 cargo 依赖图编译期拒绝（生产依赖面无根包，dev-dependency 环只
//! 覆盖测试目标；机器面负向核对住结构守门的 crate 依赖方向）。
//!
//! **测试实例纪律（dev-dependency 环双实例，ledger-transaction/#1092 同款）**：
//! `cargo test -p ledger-savings-goal` 的依赖图内存在本 crate 的两份实例——被测
//! 本实例与根包图内实例（静态与类型身份分离）。本域行为路径不消费任何域间接缝
//! 注册静态，域单测直接驱动本实例；经壳层的旅程由根包侧三层测试（命令集成、
//! e2e BDD）走根包图实例覆盖，断言与场景文本零改动。

mod crud;
mod model;
mod progress;

/// 域 API 再导出：调用面用域语言短名（`savings_goal::create_savings_goal` 等），
/// 先例 `policy` / `item` / `physical_asset` 入口再导出。
pub use crud::{create_savings_goal, update_savings_goal};
pub use model::{
    SavingsGoal, SavingsGoalInput, SavingsGoalProgress, SavingsGoalStatus, SavingsGoalUpdateInput,
};
pub use progress::list_savings_goal_progress;

#[cfg(test)]
mod tests;
