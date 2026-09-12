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

//! 核心交易域 crate（Transaction，spec #1086 / issue #1092 自根包域目录拆出，
//! P2 首个拆出的底层业务域 crate；域目录化先例 #403，ADR-0056）：交易写入、
//! 行为编排、读取、搜索与金额口径的单一权威，全部业务域可依赖的最底层域。
//!
//! 接缝：
//! - [`amount`]（口径权威）：kind 枚举真源 + kind→度量矩阵 + 本位币折算。
//! - [`base_currency_seam`]（交易×币种接缝，issue #1092）：本位币基准读取注册点。
//! - [`batch`]（批量编排权威）：批量事务、幂等键/内容哈希去重判定与批次汇总日志（`TransactionBatch::run`）。
//! - [`behavior`]（行为层编排权威）：create / update / delete 三编排入口；创建与
//!   修改的写入协议单正文（Local / Replay 两形态，ADR-0105）承载顺序契约、
//!   守卫单点、事务自持与即建商户证据；同步重放经 `replay_command` 进同一协议。
//! - [`command`]（同步命令，issue #855）：交易同步命令载荷形态与 op 产出单点（`record_local`，行为编排入口专用）。
//! - [`read`]（读取权威）：交易列表（过滤/排序/分页）与单笔读取。
//! - [`search`]（搜索权威）：SQL 候选流式扫描 + 统一模糊搜索契约过滤与分页。
//! - [`search_text`]（统一模糊搜索语义）：拼音首字母、子序列判定与词条匹配纯函数（ADR-0027）。
//! - [`model`]：域集中模型——交易全量类型（#423 模型域化随域归位），经本入口
//!   逐类型再导出（禁止 glob）；
//! - [`writer`]（写入权威）：归一化 + 全列映射 + 审计字段生成（issue #55 落地）。
//! - [`investment_seam`]（交易×投资接缝，issue #1092）：投资 kind 写路径计划装配/
//!   副作用与读路径投影（标的反查、转换两腿）的注册点与计划契约——投资语义
//!   全部归投资域实现侧，壳层启动接线。
//! - [`merchant_seam`]（交易×商户接缝，issue #1092）：商户名归一化（先查/后建）
//!   钩子组注册点。
//! - [`write_effects`]（写路径副作用接缝，issue #1090 / spec #1086 形态推广）：
//!   受影响账户余额重算的注册点与委派单点——本域只承诺调用时机，实现由账户域
//!   提供、壳层启动时接线（下层定义注册点、上层注册实现）。
//!
//! 依赖方向（spec #1086 / ADR-0112）：本域消费基础设施与同步协议
//! （`ledger-infra` / `ledger-sync-protocol`），对根包（壳层）与任何业务域零依赖
//! ——迁移前对账户/定时计划（#1090）与投资/商户/币种/物品/保单/账户（#1092）的
//! 全部残留边已按挂载点反转收敛：本域只定义注册点，实现由各提供域装入、壳层
//! 启动时接线，未注册即码化错误（失败可见，不静默丢副作用）。
//!
//! 依赖方向由编译期强制（issue #1092 AC3）：生产依赖面（lib 目标）没有根包
//! `tauri_app_lib` 与任何业务域，构造对它们的引用即编译失败；dev-dependency 环
//! 只覆盖测试目标，不构成生产环（本 crate Cargo.toml 注释留痕）。机器面负向
//! 核对住结构守门的 crate 依赖方向（`scripts/check-structure.ts` CRATES +
//! `check-structure.test.ts` 负向夹具）——本 crate 不能用 `use tauri_app_lib::…`
//! 的 compile_fail 文档用例承载该负向例：dev-dependency 对 doctest 可见，会击穿
//! 该形态（先例说明见 ledger-backup crate 根文档与 sync-protocol crate 的
//! Cargo.toml 注释）。
//!
//! **测试实例纪律（dev-dependency 环双实例）**：`cargo test -p ledger-transaction`
//! 的依赖图内存在本 crate 的两份实例——被测本实例与根包图内实例（tauri-app 经
//! 生产依赖携带，静态与类型身份分离，ledger-backup/ledger-infra 同款）。接缝
//! 注册静态由测试工厂（`tauri_app_lib::test_support::open`）接在根包图实例上，
//! 故走接缝的行为路径测试一律经 `tauri_app_lib::transaction::…` 驱动（同一份
//! 源码）；仅纯函数与守门用例（不读注册静态）直接驱动本实例。带类型签名的钩子
//! 无法跨实例注册（名义类型不等价），这是测试实例划分的硬约束。

pub mod amount;
pub mod base_currency_seam;
pub mod batch;
pub mod behavior;
pub mod command;
pub mod funding;
pub mod investment_seam;
pub mod merchant_seam;
pub mod read;
pub mod search;
pub mod search_text;
pub mod write_effects;
pub mod writer;

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
pub use batch::{
    BatchOutcome, DedupIdentity, TransactionBatch, compute_dedup_hash, dedup_identity,
};
/// 同步重放（跨 crate 消费：sync_engine::apply_ops 经本接缝执行外来命令，
/// ADR-0033）。嵌套感知事务原语已归位基础设施 `db::tx_scope`（issue #1013）。
pub use behavior::replay_command;
pub use behavior::{
    TransactionWrite, create, create_transaction, create_transaction_internal, delete,
    delete_transaction, delete_transaction_internal, update, update_transaction,
    update_transaction_internal,
};
pub use command::{ConvertCommandFields, InvestmentCommandFields, TransactionCommand};
pub use funding::validate_funding_account;
pub use read::{
    get_transaction, get_transaction_internal, list_transactions, list_transactions_internal,
};
pub use search::{repair_note_pinyin, search_transactions, search_transactions_internal};
pub use search_text::{
    is_subsequence, pinyin_initials, split_terms, term_matches, term_matches_text,
};

#[cfg(test)]
mod tests;
