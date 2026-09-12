//! 交易同步命令（issue #855 / ADR-0091）：op 载荷的交易域形态与产出单点。
//!
//! - **载荷形态**（[`TransactionCommand`]）：创建/修改携带实体 id + 归一化行
//!   （`NormalizedTransaction`，含源端折算结果 `amount_native_cents`——折算在
//!   记账端一次性完成并随 op 携带，重放不依赖本地汇率表，ADR-0091 决策 3）；
//!   删除只需实体 id（重放端按行现状执行同一编排协议）。折算的语义输入
//!   （raw 金额、币种）即行内字段随 op 在场，汇率输入不显式携带——确定性由
//!   结果携带保证；若后续审计需要精确汇率，追加可选字段即可（只增不改）。
//!   投资 kind 的语义字段随 [`InvestmentCommandFields`] 携带（buy/sell 重放执行
//!   见 `behavior::replay_command`）；转换 kind 的语义字段与源端结转成本随
//!   [`ConvertCommandFields`] 携带（issue #980，重放端本地重建 FIFO 快照）。
//!   份额调整 kind 的语义字段与源端**最终持仓 / 批次总成本**两个比对锚点随
//!   [`SplitCommandFields`] 携带（issue #1053，重放端本地重建批次重述并比对，
//!   不一致显式挂起；逐批次重述结果不随载荷携带，ADR-0106 决策 9）。
//!   **只增不改**：字段演进只追加可选成员，旧日志可在新 schema 重放。
//! - **产出单点**（[`record_local`]）：行为编排三入口（create / update /
//!   delete）成功后各自调用一次，op 产出不散落各写路径——IPC/HTTP/批量导入/
//!   余额调整等写路径全部经行为编排入口收敛，op 随入口事务提交/回滚。
//!
//! 重放执行（`replay_command`）与本地写入共用同一编排协议（写入协议，Local /
//! Replay 两形态，ADR-0105），见 [`super::behavior`]。

use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

use super::model::NormalizedTransaction;

/// 交易同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TransactionCommand {
    /// 创建交易：实体 id 与归一化行随命令携带（重放端不得重新生成 id 或折算）。
    Create {
        id: String,
        row: NormalizedTransaction,
        /// 投资 kind（buy/sell/dividend）的语义字段；其余 kind 为 None。
        investment: Option<InvestmentCommandFields>,
        /// 转换 kind（convert）的语义字段与源端算定的结转成本；其余 kind 与
        /// 旧版本设备产出的载荷（该成员缺省）为 None（ADR-0099 决策 6）。
        #[serde(default)]
        convert: Option<ConvertCommandFields>,
        /// 份额调整 kind（split）的语义字段与源端比对锚点；其余 kind 与旧版本
        /// 设备产出的载荷（该成员缺省）为 None（ADR-0106 决策 9 / issue #1053）。
        #[serde(default)]
        split: Option<SplitCommandFields>,
    },
    /// 全字段替换修改（与本地修改同语义）：实体 id 与归一化后的新行随命令携带。
    Update {
        id: String,
        row: NormalizedTransaction,
        /// 投资 kind（buy/sell/dividend）的语义字段；其余 kind 为 None。
        investment: Option<InvestmentCommandFields>,
        /// 转换 kind（convert）的语义字段与源端算定的结转成本；其余 kind 与
        /// 旧版本设备产出的载荷（该成员缺省）为 None（ADR-0099 决策 6）。
        #[serde(default)]
        convert: Option<ConvertCommandFields>,
        /// 份额调整 kind（split）的语义字段与源端比对锚点；其余 kind 与旧版本
        /// 设备产出的载荷（该成员缺省）为 None（ADR-0106 决策 9 / issue #1053）。
        #[serde(default)]
        split: Option<SplitCommandFields>,
    },
    /// 删除交易（软删除）：实体 id 足够——重放端读行现状（kind 守卫、账户引用）
    /// 执行与本地删除同一协议。
    Delete { id: String },
}

impl TransactionCommand {
    /// 命令指向的实体 id（创建/修改随行携带，删除即目标 id）。
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            TransactionCommand::Create { id, .. } | TransactionCommand::Update { id, .. } => id,
            TransactionCommand::Delete { id } => id,
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识；op 产出直呼协议面（环依赖由 crate 依赖图断开）。
impl SyncCommand for TransactionCommand {
    const ENTITY: &'static str = "transaction";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

/// 转换 kind（convert）的命令字段（ADR-0099 决策 6 / issue #980）：随 op 携带的
/// 语义输入与源端算定的结转成本。
///
/// 重放端本地重建 FIFO 快照（既有「成本随命令携带、快照本端重建」先例，
/// ADR-0091 决策 3）：`carried_cost_cents` 是源端逐批次消耗算出的结转成本合计
/// （等于行金额锚点），作为转入批次成本与逐批次消耗合计的权威比对基准——本地
/// 重建的合计与其不一致即本地快照发散，重放显式失败挂起，不静默落出错误成本基础。
///
/// `instrument_id`（转出标的）不在 ADR-0099 决策 6 的字段清单内，但载荷自包含
/// 所需：转出标的既非行字段、也无法从其余字段推出，缺它则重放端无从取 FIFO 批次
/// 与落 `security_transactions` 转出行。按「载荷只增不改」纪律随本票补入。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvertCommandFields {
    /// 转出标的（`security_transactions.instrument_id`）。
    pub instrument_id: String,
    /// 转出份额（FIFO 消耗量）。
    pub quantity: f64,
    /// 转入标的（`security_transactions.to_instrument_id`）。
    pub to_instrument_id: String,
    /// 转入份额（转入批次建仓量）。
    pub to_quantity: f64,
    /// 转出金额（确认单权威，整数分）。
    pub out_amount_cents: i64,
    /// 转入金额（确认单权威，整数分）。
    pub in_amount_cents: i64,
    /// 手续费（整数分）。
    pub fee_cents: i64,
    /// 结转成本（源端算定，整数分）：行金额锚点与转入批次成本来源。
    pub carried_cost_cents: i64,
}

/// 份额调整 kind（split）的命令字段（ADR-0106 决策 9 / issue #1053）：随 op 携带的
/// 语义输入与源端算定的**总量比对锚点**。
///
/// 与 convert 的差别（ADR-0106 决策 9）：重述是「当前批次快照 + Δ」的纯函数，
/// 重放端按 op 序重建的批次快照与源端一致，故**不携带逐批次重述结果**——只携带 Δ
/// 与两个总量（最终持仓、批次总成本）。重放端以本地快照独立重建重述，两个总量与其
/// 不一致即本地快照发散（前序 op 未达、载荷被篡改），显式失败挂起，不静默错账。
///
/// 旧版本设备产出的 split op 缺本成员（`#[serde(default)]` 缺省即 None）：重放端
/// 无比对锚点可用，按既有挂起机制码化挂起（`transaction.split-fields-missing`），
/// 不静默落出未经校验的重述（同 convert 的旧载荷处置，ADR-0099 决策 6）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitCommandFields {
    /// 调整标的（`security_transactions.instrument_id`）。
    pub instrument_id: String,
    /// 带符号份额增量 Δ（绝对增量语义，`+` = 折算/结转/送股、`−` = 缩股）。
    pub delta_quantity: f64,
    /// 源端重述后总持仓（Σ remaining_after）：重放端本地重建值须与之一致。
    pub final_quantity: f64,
    /// 源端批次总成本（分，重述精确不变）：重放端本地重建值须与之一致。
    pub total_cost_cents: i64,
}

/// 投资 kind（buy/sell/dividend）的命令字段：随 op 携带的语义输入与派生结果。
/// 数量/单价/手续费是 prepare 的算定输入；买入的每份成本是 prepare 单次舍入
/// 的派生结果（源端折算随行，ADR-0091 决策 3），重放端直接落批次、不重算
///（重算需读标的类型，属本地状态）；卖出无此概念（批次成本随买方批次在本端
/// 已就位）。**现金分红（ADR-0109）只消费 `instrument_id`**——分红无份额 / 单价 /
/// 手续费，其余成员是占位零值、重放端不读；现金腿随归一化行携带。重放执行见
/// `behavior::replay_command`（#861 起）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestmentCommandFields {
    pub instrument_id: String,
    /// 成交数量（份，可含小数）。
    pub quantity: f64,
    /// 成交单价（万分之一元，价格刻度见 ADR-0038）。
    pub price_cents: i64,
    /// 手续费（整数分）。
    pub fee_cents: i64,
    /// 买入每份成本（万分之一元，含费用摊薄单次舍入，ADR-0038）；卖出为 None。
    pub cost_per_unit_cents: Option<i64>,
}

/// op 产出接缝（交易域集中单点）：本地交易写成功后追加一条 op 进本机 OpLog。
///
/// 仅行为编排入口（`transaction::behavior` 的 create / update / delete 协议）
/// 调用；随编排事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: TransactionCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}
