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

//! 核心交易域 crate（Transaction，spec #1086 / issue #1092）：交易写入、读取、搜索
//! 与金额口径的单一权威，全部业务域可依赖的最底层域。
//!
//! **四区唯一地图（ADR-0113）**：`amount/` `model` `command` `search_text` 共享语义；
//! `seams/` 跨域接缝；`write/` 写路径；`read/` 读路径。允许依赖方向唯一——写路径 /
//! 读路径 → 跨域接缝 → 共享语义，写读两径互不依赖。**新代码一律走区路径**；下方
//! crate 根扁平再导出只是壳层兼容面，不是权威入口。
//!
//! `pub use` 兼容面（存量调用点零改动）覆盖金额口径、域模型、批量、写入协议、同步
//! 命令载荷与读取 / 搜索入口；接缝注册点与行写入类型经区路径消费（无旧模块别名转发）。
//!
//! 依赖方向（spec #1086 / ADR-0112）：本域消费 `ledger-infra` / `ledger-sync-protocol`，
//! 对根包（壳层）与任何业务域零依赖——六向残留边已按挂载点反转收敛（ADR-0112 决策 5），
//! 本域只定义注册点、各提供域装入、壳层启动接线，未注册即码化错误。生产依赖面无根包
//! 与业务域，构造引用即编译失败；dev-dependency 环只覆盖测试目标（本 crate Cargo.toml
//! 留痕），反向引用负向核对住结构守门的 CRATES 依赖方向（本 crate 不能用
//! `use tauri_app_lib::…` compile_fail 承载：dev-dependency 对 doctest 可见）。
//!
//! **测试实例纪律（dev-dependency 环双实例）**：`cargo test -p ledger-transaction`
//! 依赖图内存在本 crate 两份实例——被测本实例与根包图内实例。接缝注册静态由测试工厂
//! （`tauri_app_lib::test_support::open`）接在根包图实例上，故走接缝的行为路径测试一律
//! 经 `tauri_app_lib::transaction::…` 驱动；仅纯函数与守门用例（不读注册静态）直接驱动
//! 本实例。带类型签名的钩子无法跨实例注册（名义类型不等价），这是测试实例划分的硬约束。

pub mod amount;
pub mod command;
pub mod read;
pub mod seams;
pub mod search_text;
pub mod write;

/// 域集中模型（#423 模型域化随域归位，样板先例：`investment::model`）：交易
/// 全量类型集中本文件，经域路径逐类型再导出（禁止 glob），消费方经域路径
/// 显式 import。
mod model;

pub use model::{
    ConvertFields, CreateTransactionResult, NormalizedTransaction, NotePinyinRepairFailure,
    NotePinyinRepairReport, NotePinyinRepairStage, Transaction, TransactionBatchInput,
    TransactionInput, TransactionListFilter, TransactionListResult, TransactionSearchResult,
    TransactionSource, TransactionSourceKind, TransactionSourceStatus, UpdateTransactionInput,
};

pub use amount::{
    Measure, TransactionKind, TransferSide, account_flow_expr, contributing_kinds,
    contributing_kinds_sql, convert_to_native, default_currency_code, expense_gross_expr,
    expense_net_expr, income_net_expr, policy_inflow_expr, policy_premium_expr, refund_gross_expr,
    signed_amount,
};
pub use command::{ConvertCommandFields, InvestmentCommandFields, TransactionCommand};
pub use read::search::{repair_note_pinyin, search_transactions, search_transactions_internal};
pub use read::{
    get_transaction, get_transaction_internal, list_transactions, list_transactions_internal,
};
pub use search_text::{
    is_subsequence, pinyin_initials, split_terms, term_matches, term_matches_text,
};
pub use write::batch::{
    BatchOutcome, DedupIdentity, TransactionBatch, compute_dedup_hash, dedup_identity,
};
pub use write::funding::validate_funding_account;
/// 同步重放（跨 crate 消费：sync_engine::apply_ops 经本接缝执行外来命令，
/// ADR-0033）。嵌套感知事务原语已归位基础设施 `db::tx_scope`（issue #1013）。
pub use write::protocol::replay_command;
pub use write::protocol::{
    TransactionWrite, create, create_transaction, create_transaction_internal, delete,
    delete_transaction, delete_transaction_internal, update, update_transaction,
    update_transaction_internal,
};

#[cfg(test)]
mod tests;
