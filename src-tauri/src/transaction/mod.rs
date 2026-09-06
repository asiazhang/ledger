//! 交易领域模块（spec #52；#403 核心交易域收口 ADR-0056）：交易写入、行为编排、读取、搜索与金额口径的单一权威。
//!
//! 接缝：
//! - [`amount`]（口径权威）：kind 枚举真源 + kind→度量矩阵 + 本位币折算。
//! - [`batch`]（批量编排权威）：批量事务、幂等键/内容哈希去重判定与批次汇总日志（`TransactionBatch::run`）。
//! - [`behavior`]（行为层编排权威）：create / update / delete 三编排入口、嵌套事务感知、plan/apply 副作用分派与即建商户证据。
//! - [`read`]（读取权威）：交易列表（过滤/排序/分页）与单笔读取。
//! - [`search`]（搜索权威）：SQL 候选流式扫描 + 统一模糊搜索契约过滤与分页。
//! - [`search_text`]（统一模糊搜索语义）：拼音首字母、子序列判定与词条匹配纯函数（ADR-0027）。
//! - [`model`]：域集中模型——交易全量类型（#423 模型域化随域归位），经本入口
//!   逐类型再导出（禁止 glob）；
//! - [`writer`]（写入权威）：归一化 + 全列映射 + 审计字段生成（issue #55 落地）。
//!
//! 依赖方向恒为「壳层 → transaction → 基础设施」，本模块不反向依赖壳层。

pub mod amount;
pub mod batch;
pub mod behavior;
pub mod read;
pub mod search;
pub mod search_text;
pub mod writer;

/// 域集中模型（#423 模型域化随域归位，样板先例：`investment::model`）：交易
/// 全量类型集中本文件，经域路径逐类型再导出（禁止 glob），消费方经域路径
/// 显式 import。
mod model;

pub use model::{
    CreateTransactionResult, NormalizedTransaction, NotePinyinRepairFailure,
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
pub use behavior::{
    TransactionWrite, create, create_transaction, create_transaction_internal, delete,
    delete_transaction, delete_transaction_internal, update, update_transaction,
    update_transaction_internal,
};
pub use read::{
    get_transaction, get_transaction_internal, list_transactions, list_transactions_internal,
};
pub use search::{repair_note_pinyin, search_transactions, search_transactions_internal};
pub use search_text::{
    is_subsequence, pinyin_initials, split_terms, term_matches, term_matches_text,
};

#[cfg(test)]
mod tests;
