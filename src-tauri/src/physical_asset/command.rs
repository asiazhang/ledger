//! 实物资产同步命令（issue #860 / ADR-0091）：op 载荷的实物资产域形态、产出
//! 单点与重放分派。
//!
//! - **载荷形态**（[`PhysicalAssetCommand`]）：建档携带实体 id + 资产语义行 +
//!   **首条估值行**（估值行 id 随行——「同日按插入序」以 UUID v7 主键时间序裁决，
//!   跨端携带同 id 即同一裁决）；编辑携带解决后的语义行；估值更新携带新估值行
//!   （只追加不改写，历史行无实体指向、不参与同实体 LWW——并发估值全部存活，
//!   当前值由读口径自然裁决）；处置携带落定日期与价格；删除只需实体 id。
//!   **只增不改**。
//! - **产出单点**（[`record_local`]）：实物资产写编排入口成功后调用，op 随写
//!   事务提交/回滚（建档两表写入与 op 同事务）。
//! - **重放执行**（[`replay_command`]）：与本地写同一执行协议（名称、成对、
//!   金额、币种、日期守卫原样生效，币种缺失即挂起），不产出 op。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::sync_engine::{DomainCommand, record_local as record_op};

/// 估值行载荷（只追加历史行；id 随行保序）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValuationCommandRow {
    pub id: String,
    pub valuation_date: String,
    pub amount_cents: i64,
    pub currency_code: String,
}

/// 实物资产同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PhysicalAssetCommand {
    /// 建档：实体 id、资产语义行与首条估值行随命令携带（重放端不得重新生成 id）。
    Create {
        id: String,
        name: String,
        purchase_date: Option<String>,
        purchase_price_cents: Option<i64>,
        purchase_currency_code: Option<String>,
        first_valuation: ValuationCommandRow,
    },
    /// 编辑档案（名称 + 购买信息为解决后的落定值；估值不经编辑变更）。
    Update {
        id: String,
        name: String,
        purchase_date: Option<String>,
        purchase_price_cents: Option<i64>,
        purchase_currency_code: Option<String>,
    },
    /// 追加估值历史行（当前估值 = 最新一条，由读口径裁决；无实体指向）。
    UpdateValuation {
        asset_id: String,
        valuation: ValuationCommandRow,
    },
    /// 处置（状态标记非删除；再处置 = 修正，与本地同语义）。
    Dispose {
        id: String,
        disposal_date: String,
        disposal_price_cents: Option<i64>,
        disposal_currency_code: Option<String>,
    },
    /// 删除（软删除；估值历史保留）。
    Delete { id: String },
}

impl PhysicalAssetCommand {
    /// 命令指向的实体键（LWW 裁决域 = 单件资产）；估值追加为只追加历史行，无实体
    /// 指向（并发估值全部存活，不参与同实体 LWW）。实体标签不在此返回——由同步域
    /// 重放注册表单源组装（ADR-0101 勘误 3）。
    pub(crate) fn subject(&self) -> Option<&str> {
        match self {
            PhysicalAssetCommand::Create { id, .. }
            | PhysicalAssetCommand::Update { id, .. }
            | PhysicalAssetCommand::Dispose { id, .. }
            | PhysicalAssetCommand::Delete { id } => Some(id),
            PhysicalAssetCommand::UpdateValuation { .. } => None,
        }
    }
}

/// op 产出接缝（实物资产域集中单点）：本地写成功后追加一条 op 进本机 OpLog。
///
/// 仅实物资产写编排入口（`physical_asset::crud` 的五个写协议）调用；随编排事务
/// 提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: PhysicalAssetCommand) -> Result<()> {
    record_op(conn, DomainCommand::PhysicalAsset(command))?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：按动作转发到与本地写同一执行协议。
pub(crate) fn replay_command(conn: &Connection, command: &PhysicalAssetCommand) -> Result<()> {
    match command {
        PhysicalAssetCommand::Create {
            id,
            name,
            purchase_date,
            purchase_price_cents,
            purchase_currency_code,
            first_valuation,
        } => super::crud::replay_create(
            conn,
            id,
            name,
            purchase_date.as_deref(),
            *purchase_price_cents,
            purchase_currency_code.as_deref(),
            first_valuation,
        ),
        PhysicalAssetCommand::Update {
            id,
            name,
            purchase_date,
            purchase_price_cents,
            purchase_currency_code,
        } => super::crud::replay_update(
            conn,
            id,
            name,
            purchase_date.as_deref(),
            *purchase_price_cents,
            purchase_currency_code.as_deref(),
        ),
        PhysicalAssetCommand::UpdateValuation {
            asset_id,
            valuation,
        } => super::crud::replay_valuation(conn, asset_id, valuation),
        PhysicalAssetCommand::Dispose {
            id,
            disposal_date,
            disposal_price_cents,
            disposal_currency_code,
        } => super::crud::replay_dispose(
            conn,
            id,
            disposal_date,
            *disposal_price_cents,
            disposal_currency_code.as_deref(),
        ),
        PhysicalAssetCommand::Delete { id } => super::crud::replay_delete(conn, id),
    }
}
