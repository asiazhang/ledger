//! 交易同步命令（issue #855 / ADR-0091）：op 载荷的交易域形态与产出单点。
//!
//! - **载荷形态**（[`TransactionCommand`]）：创建/修改携带实体 id + 归一化行
//!   （`NormalizedTransaction`，含源端折算结果 `amount_native_cents`——折算在
//!   记账端一次性完成并随 op 携带，重放不依赖本地汇率表，ADR-0091 决策 3）；
//!   删除只需实体 id（重放端按行现状执行同一编排协议）。投资 kind 的语义字段
//!   随 [`InvestmentCommandFields`] 携带（其重放执行待 #861，先随日志留痕）。
//!   **只增不改**：字段演进只追加可选成员，旧日志可在新 schema 重放。
//! - **产出单点**（[`record_local`]）：行为编排三入口（create / update /
//!   delete）成功后各自调用一次，op 产出不散落各写路径——IPC/HTTP/批量导入/
//!   余额调整等写路径全部经行为编排入口收敛，op 随入口事务提交/回滚。
//!
//! 重放执行（`replay_command`）与本地写入共用同一编排协议，见
//! [`super::behavior`] 的重放形态函数。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::sync_engine::{DomainCommand, record_local as record_op};

use super::model::NormalizedTransaction;

/// 交易同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TransactionCommand {
    /// 创建交易：实体 id 与归一化行随命令携带（重放端不得重新生成 id 或折算）。
    Create {
        id: String,
        row: NormalizedTransaction,
        /// 投资 kind（buy/sell）的语义字段；普通 kind 为 None。
        investment: Option<InvestmentCommandFields>,
    },
    /// 全字段替换修改（与本地修改同语义）：实体 id 与归一化后的新行随命令携带。
    Update {
        id: String,
        row: NormalizedTransaction,
        /// 投资 kind（buy/sell）的语义字段；普通 kind 为 None。
        investment: Option<InvestmentCommandFields>,
    },
    /// 删除交易（软删除）：实体 id 足够——重放端读行现状（kind 守卫、账户引用）
    /// 执行与本地删除同一协议。
    Delete { id: String },
}

/// 投资 kind（buy/sell）的命令字段：随 op 携带的语义输入，供审计留痕与后续
/// 投资命令重放（#861）消费；v1 重放对投资命令显式码化拒绝（fail loud）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestmentCommandFields {
    pub instrument_id: String,
    /// 成交数量（份，可含小数）。
    pub quantity: f64,
    /// 成交单价（万分之一元，价格刻度见 ADR-0038）。
    pub price_cents: i64,
    /// 手续费（整数分）。
    pub fee_cents: i64,
}

/// op 产出接缝（交易域集中单点）：本地交易写成功后追加一条 op 进本机 OpLog。
///
/// 仅行为编排入口（`transaction::behavior` 的 create / update / delete 协议）
/// 调用；随编排事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: TransactionCommand) -> Result<()> {
    record_op(conn, DomainCommand::Transaction(command))?;
    Ok(())
}
