//! 域集中模型区（共享语义，issue #423 随域归位；ADR-0113 决策 2/8）。
//!
//! 唯一地图：`transaction`（交易实体与读模型）/ `input`（写入入参）/ `normalized`
//! （归一化行）/ `filter`（列表过滤）/ `repair`（拼音回填报告）。不变量：全部类型经
//! `crate::model` 逐类型再导出（禁止 glob，ADR-0059 决策 6）；到写路径 `NormalizedRow`
//! 的转换 impl 不在此（ADR-0113 决策 3.2）。ADR 指针：ADR-0059 / ADR-0113 决策 3.2。
//!
//! 本文件只做声明与逐项再导出（ADR-0113 决策 5），不含逻辑。

mod filter;
mod input;
mod normalized;
mod repair;
mod transaction;

pub use filter::TransactionListFilter;
pub use input::{
    CreateTransactionResult, TransactionBatchInput, TransactionInput, UpdateTransactionInput,
};
pub use normalized::NormalizedTransaction;
pub use repair::{NotePinyinRepairFailure, NotePinyinRepairReport, NotePinyinRepairStage};
pub use transaction::{
    ConvertFields, Transaction, TransactionListResult, TransactionSearchResult, TransactionSource,
    TransactionSourceKind, TransactionSourceStatus,
};
