//! 物品同步命令（issue #860 / ADR-0091）：op 载荷的物品域形态、产出单点与重放分派。
//!
//! - **载荷形态**（[`ItemCommand`]）：创建/修改携带实体 id + 解决后的语义行
//!   （含**源端折算结果** `cost_native_cents`——Amount 接缝在记账端一次性完成，
//!   重放不依赖本地汇率表，ADR-0091 决策 3）；处置携带落定日期与残值；删除只需
//!   实体 id。**只增不改**。
//! - **产出单点**（[`record_local`]）：物品写编排入口（创建 / 修改 / 处置 / 删除）
//!   成功后调用，op 随写事务提交/回滚。
//! - **重放执行**（[`replay_command`]）：与本地写同一执行协议（溯源守卫、处置
//!   日期守卫原样生效，关联交易缺失或溯源坑位被占即挂起待裁决），不产出 op。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::sync_engine::{DomainCommand, record_local as record_op};

/// 物品命令行载荷（语义字段；簿记戳不随行携带）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemCommandRow {
    pub name: String,
    pub purchase_date: String,
    pub total_cost_cents: i64,
    pub currency_code: String,
    /// 源端折算结果（ADR-0091 决策 3）：重放端直接落库，不重折算。
    pub cost_native_cents: i64,
    /// 溯源指针（词汇表 source_transaction_id，代码过渡列名 purchase_transaction_id）。
    pub purchase_transaction_id: Option<String>,
    pub note: Option<String>,
}

/// 物品同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ItemCommand {
    /// 创建物品：实体 id 与语义行随命令携带（重放端不得重新生成 id 或折算）。
    Create { id: String, row: ItemCommandRow },
    /// 修改物品（解决后的语义行，溯源只增不减语义已在源端解决为落定指针）。
    Update { id: String, row: ItemCommandRow },
    /// 处置物品（状态流转；再处置 = 修正，与本地同语义）。
    Dispose {
        id: String,
        disposal_date: String,
        residual_value_cents: Option<i64>,
    },
    /// 删除物品（软删除；溯源坑位随之释放，与本地同语义）。
    Delete { id: String },
}

impl ItemCommand {
    /// 命令指向的实体 id（LWW 裁决域 = 单件物品）。
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            ItemCommand::Create { id, .. }
            | ItemCommand::Update { id, .. }
            | ItemCommand::Dispose { id, .. }
            | ItemCommand::Delete { id } => id,
        }
    }
}

/// op 产出接缝（物品域集中单点）：本地物品写成功后追加一条 op 进本机 OpLog。
///
/// 仅物品写编排入口（`item::domain` 的创建 / 修改 / 处置 / 删除协议）调用；随
/// 编排事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: ItemCommand) -> Result<()> {
    record_op(conn, DomainCommand::Item(command))?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：按动作转发到与本地写同一执行协议。
pub(crate) fn replay_command(conn: &Connection, command: &ItemCommand) -> Result<()> {
    match command {
        ItemCommand::Create { id, row } => super::domain::replay_create(conn, id, row),
        ItemCommand::Update { id, row } => super::domain::replay_update(conn, id, row),
        ItemCommand::Dispose {
            id,
            disposal_date,
            residual_value_cents,
        } => super::domain::replay_dispose(
            conn,
            id,
            &super::model::ItemDisposeInput {
                disposal_date: disposal_date.clone(),
                residual_value_cents: *residual_value_cents,
            },
        ),
        ItemCommand::Delete { id } => super::domain::replay_delete(conn, id),
    }
}
