//! 归一化交易行（共享语义区，issue #855）：随同步命令搬运的行载荷。
//!
//! 职责：创建与修改共用的归一化行字段。不变量：随 op 序列化跨端搬运，含源端折算结果；
//! 到 `write::writer::NormalizedRow` 的转换 impl 归写路径（ADR-0113 决策 3.2）。
//! ADR 指针：ADR-0091 / ADR-0113 决策 3.2。陷阱：字段演进只增不改。

use serde::{Deserialize, Serialize};

use crate::amount::{FxRateSource, TransactionKind};

use crate::model::TransactionFundingInput;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedTransaction {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    /// 折算来源留痕（issue #1548 / ADR-0011 修订，V029）：源端折算使用的汇率值与
    /// 来源（序列命中 / 调用方显式给定，#1549 接入），随 op 搬运使重放端收敛到
    /// 与源端一致的行。未折算（与本位币同币种、无现金腿的 split）为 `None`。
    /// **只增不改**：`#[serde(default)]` 使旧版本载荷（成员缺省）反序列化为 `None`，
    /// 与该行「写入时点早于留痕功能」的库内 NULL 语义一致。
    #[serde(default)]
    pub fx_rate_used: Option<f64>,
    #[serde(default)]
    pub fx_rate_source: Option<FxRateSource>,
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    /// 可选出资账户（issue #935 / ADR-0096）：随归一化行落库与随 op 搬运。
    pub funding_account_id: Option<String>,
    /// 出资项分解（issue #1860 / ADR-0138）：分解行随行搬运（同步重放同语义落
    /// 子表）；分解行本字段非空且 `account_id` 为 `None`。`#[serde(default)]` 使
    /// 旧版本载荷（成员缺省）反序列化为空——只增不改。
    #[serde(default)]
    pub funding: Vec<TransactionFundingInput>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    /// 来源订单号（issue #1862 / ADR-0138 决策 9，V035）：随归一化行落库与随 op 搬运
    /// （重放端同语义落列）。`#[serde(default)]` 使旧版本载荷（成员缺省）反序列化为
    /// `None`——只增不改。
    #[serde(default)]
    pub source_order_no: Option<String>,
    pub date: String,
}
