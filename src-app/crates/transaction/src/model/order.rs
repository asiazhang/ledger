//! 订单汇总读模型（共享语义区，issue #1862 / ADR-0138 决策 9）。
//!
//! 职责：详情订单区只读命令的返回类型——同单行集、行数、合计与按账户聚合的
//! 出资构成。不变量：类型经 `crate::model` 逐类型再导出（禁止 glob）。ADR 指针：
//! ADR-0138 决策 9。
//!
//! 口径注记：`total_amount_cents` 是同单各行原始币种金额的直和（对照回单核对
//! 的语义）；`currency_code` 仅在行集币种唯一时携带（单回单订单的常态），混合
//! 币种为 `None`——不静默混算，由前端回落展示币种格式化。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 出资构成按账户聚合的条目（issue #1862 / ADR-0138 决策 9）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct OrderAccountContribution {
    /// 出资账户 id。
    pub account_id: String,
    /// 该账户在本订单的出资合计（正整数分）。
    pub amount_cents: i64,
}

/// 订单汇总（issue #1862 / ADR-0138 决策 9）：按来源订单号取同单行集的只读投影。
///
/// 出资构成聚合口径：分解行按出资项逐条计入；单出资行按 `account_id` 整额计入
/// （`to_account_id` / `funding_account_id` 端不计——组合支付订单行是 expense /
/// income / refund，转账与投资行的出资端语义不同）。顺序：先行序（日期升序）后
/// 行内出资顺序位，同账户多条合并。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TransactionOrderSummary {
    /// 来源订单号（查询键原样返回）。
    pub source_order_no: String,
    /// 同单未删除行数。
    pub row_count: i64,
    /// 同单各行原始币种金额合计（整数分直和）。
    pub total_amount_cents: i64,
    /// 同单币种（行集币种唯一时携带；混合币种为 `None`，不静默混算）。
    pub currency_code: Option<String>,
    /// 按账户聚合的出资构成（单账户订单 1 条、多账户订单 ≥2 条）。
    pub accounts: Vec<OrderAccountContribution>,
    /// 同单各行明细（日期升序，读回契约与列表同形——分解行随行携带 fundings）。
    pub items: Vec<crate::model::Transaction>,
}
