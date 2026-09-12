//! 归一化交易行（共享语义区，issue #855）：随同步命令搬运的行载荷。
//!
//! 职责：创建与修改共用的归一化行字段。不变量：随 op 序列化跨端搬运，含源端折算结果；
//! 到 `write::writer::NormalizedRow` 的转换 impl 归写路径（ADR-0113 决策 3.2）。
//! ADR 指针：ADR-0091 / ADR-0113 决策 3.2。陷阱：字段演进只增不改。

use serde::{Deserialize, Serialize};

use crate::amount::TransactionKind;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedTransaction {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub account_id: String,
    pub to_account_id: Option<String>,
    /// 可选出资账户（issue #935 / ADR-0096）：随归一化行落库与随 op 搬运。
    pub funding_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
}
