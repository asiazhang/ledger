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

//! 定时计划领域 crate（Scheduled，issue #307 / ADR-0042；spec #1086 / issue #1098
//! 自根包域目录拆出）：定时交易计划（分期/订阅/定时转账）、期次展开与执行引擎、
//! 自动执行追补入口、订阅花费双口径与同步命令在本 crate 收口；IPC 参数解包、事务
//! 边界、命令注册和失效信号发射留在定时计划命令壳层。crate 不依赖壳层，域模型
//! 集中 `models` 模块，消费方经域路径逐类型显式 import。
//!
//! 依赖方向（spec #1086 / issue #1098 AC）：本 crate 消费基础设施与核心交易域
//!（`ledger-infra` / `ledger-transaction`——期次落库与校验经 writer 接缝、花费合计
//! 经 amount 矩阵、计划来源模型随域消费）与同步协议（`ledger-sync-protocol`——
//! 命令契约、op 产出与设备标识）。对备份域（票面允许集内）零依赖：期次落账置脏
//! 与追补触发两条边已按注册点反转收敛（挂载点③/④，ADR-0112 决策 5）——本域只
//! 定义 [`auto_run::register_after_occurrence_hook`] 注册点并经
//! [`auto_run::catch_up_hook`] 提供追补实现，备份域注册点由壳层启动时对装，未注册
//! 即日志可见（不静默丢置脏）。对根包（壳层）与同步域零依赖——重放分派住
//! sync_engine（业务域→同步域零容忍，ADR-0101 决策 4b）。依赖方向由编译期强制
//!（issue #1098 AC3）：生产依赖面（lib 目标）没有根包 `tauri_app_lib` 与备份域
//! `ledger_backup`，构造对它们的引用即编译失败；dev-dependency 环只覆盖测试目标，
//! 不构成生产环（本 crate Cargo.toml 注释留痕）。
//!
//! **测试实例纪律（dev-dependency 环双实例，ledger-transaction/#1092 同款）**：
//! `cargo test -p ledger-scheduled` 的依赖图内存在本 crate 的两份实例——被测本
//! 实例与根包图内实例（静态与类型身份分离）。本域自有注册静态是期次落账置脏钩子
//!（[`auto_run::register_after_occurrence_hook`]），由测试工厂
//!（`tauri_app_lib::test_support::open`）接在根包图实例上，故走该路径的行为测试
//!（tests/auto_run.rs）经 `tauri_app_lib::scheduled_transactions::…` 驱动；其余
//! 用例不读本 crate 注册静态（经核心交易域接缝的静态随 ledger-transaction 单实例
//! 编译共享），直接驱动本实例；建库一律经根包测试工厂（dev-dependency，ADR-0084）。

mod command;
mod engine;
mod models;
mod source;
mod spend;

pub mod auto_run;

pub use auto_run::*;
/// 同步重放（跨 crate 消费：sync_engine 分派接缝经根包再导出面执行外来命令，
/// ADR-0091。#1098 拆 crate 起 `pub(crate)`→`pub`，签名与语义不变）。
pub use command::replay_command;
pub use command::{ScheduledCommand, occurrence_transaction_id};
pub use engine::*;
pub use models::{
    CreateScheduledInput, ExecuteOccurrenceInput, InstallmentPlan, OccurrenceStatus,
    RecurrenceType, ScheduledKind, ScheduledStatus, ScheduledTransaction,
    ScheduledTransactionDetail, ScheduledTransactionOccurrence, ScheduledTransactionWithExt,
    ScheduledTransferPlan, SubscriptionPlan, UpdateStatusInput, UpdateSubscriptionInput,
};
pub use source::install_plan_source_hook;
pub use spend::*;

#[cfg(test)]
mod tests;
