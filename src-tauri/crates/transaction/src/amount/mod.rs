//! 金额口径区（共享语义，issue #54 / spec #52；ADR-0113 决策 2/8）：交易金额单一权威。
//!
//! 唯一地图：`kind`（交易类型闭集真源）/ `measure`（kind→度量矩阵与 SQL 片段）/
//! `convert`（本位币折算）/ `base_currency`（本位币读取接缝契约，决策 3.1 归本区）。
//! 不变量：本区是共享语义底层，不依赖接缝与写读路径；子模块互依同区合法。
//!
//! 本文件只做声明与逐项再导出（ADR-0113 决策 5），不含逻辑；消费方经
//! `crate::amount::…` 或本文再导出的名字使用。ADR 指针：ADR-0113 决策 3.1/8。

pub mod base_currency;
mod convert;
mod kind;
mod measure;

pub use convert::{convert_to_native, default_currency_code};
pub use kind::TransactionKind;
pub use measure::{
    Measure, TransferSide, account_flow_expr, contributing_kinds, contributing_kinds_sql,
    expense_gross_expr, expense_net_expr, income_net_expr, policy_inflow_expr, policy_premium_expr,
    refund_gross_expr, signed_amount,
};

#[cfg(test)]
mod tests;
