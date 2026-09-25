//! 储蓄目标同步命令（issue #1756 / ADR-0091 决策 2 / ADR-0101）：op 载荷的
//! 储蓄目标域形态、产出单点与重放分派。
//!
//! - **载荷形态**（[`SavingsGoalCommand`]）：创建携带实体 id + 语义行（专属账户
//!   绑定 / 名称 / 目标额 / 可选截止日；手填「计划月存」创建时恒缺省，编辑才
//!   引入——载荷只携带落定值，缺省在产出端解决）；编辑携带四字段全量替换；
//!   归档 / 取消归档只翻状态；删除只需实体 id。**只增不改**。
//! - **产出单点**（[`record_local`]）：目标写编排入口（创建 / 编辑 / 归档 /
//!   取消归档 / 删除）成功后调用，op 随写事务提交/回滚。
//! - **重放执行**（[`replay_command`]）：与本地写同一执行协议——目标金额正数 /
//!   名称非空 / 手填月存正数 / 删除余额非零禁删守卫重放侧同码生效（issue #1756
//!   AC），冲突与依赖失败挂起待裁决，不产出 op：专属账户的级联写入（建户 /
//!   改名 / 软删）由源端的账户 op 以更早时钟先行到达（被级联行的 op 由源端
//!   产出，ADR-0091），重放侧零本地 op。

use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

use super::model::SavingsGoalStatus;

/// 储蓄目标同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SavingsGoalCommand {
    /// 创建目标：实体 id、专属账户绑定与语义行随命令携带（重放端不得重新生成
    /// id；专属账户本体由源端账户 op 先行落地，重放端只落目标行）。
    Create {
        id: String,
        account_id: String,
        name: String,
        target_amount_cents: i64,
        deadline: Option<String>,
    },
    /// 编辑目标（四字段全量替换；改名联动专属账户由源端账户 op 先行到达）。
    Update {
        id: String,
        name: String,
        target_amount_cents: i64,
        deadline: Option<String>,
        planned_monthly_cents: Option<i64>,
    },
    /// 归档目标（收纳不清算：账户 / 流水 / 关联计划原样保留）。
    Archive { id: String },
    /// 取消归档目标（恢复 active 回默认列表）。
    Unarchive { id: String },
    /// 删除目标（软删；余额非零禁删守卫重放侧同码生效，冲突挂起待裁决）。
    Delete { id: String },
}

impl SavingsGoalCommand {
    /// 命令指向的实体 id（LWW 裁决域 = 单条目标）。
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            SavingsGoalCommand::Create { id, .. }
            | SavingsGoalCommand::Update { id, .. }
            | SavingsGoalCommand::Archive { id }
            | SavingsGoalCommand::Unarchive { id }
            | SavingsGoalCommand::Delete { id } => id,
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识；op 产出直呼协议面（环依赖由 crate 依赖图断开）。
impl SyncCommand for SavingsGoalCommand {
    const ENTITY: &'static str = "goal";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

/// op 产出接缝（储蓄目标域集中单点）：本地目标写成功后追加一条 op 进本机 OpLog。
///
/// 仅储蓄目标写编排入口（`savings-goal::crud` 的创建 / 编辑 / 归档 / 取消归档 /
/// 删除协议）调用；随编排事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: SavingsGoalCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：按动作转发到与本地写同一执行协议。
/// crate 拆分后跨 crate 消费：sync_engine::registry 经根包再导出面分派（#1101，
/// pub(crate)→pub，签名与语义不变，#1092 replay_command 同款）。
pub fn replay_command(conn: &Connection, command: &SavingsGoalCommand) -> Result<()> {
    match command {
        SavingsGoalCommand::Create {
            id,
            account_id,
            name,
            target_amount_cents,
            deadline,
        } => super::crud::replay_create(
            conn,
            id,
            account_id,
            name,
            *target_amount_cents,
            deadline.as_deref(),
        ),
        SavingsGoalCommand::Update {
            id,
            name,
            target_amount_cents,
            deadline,
            planned_monthly_cents,
        } => super::crud::replay_update(
            conn,
            id,
            name,
            *target_amount_cents,
            deadline.as_deref(),
            *planned_monthly_cents,
        ),
        SavingsGoalCommand::Archive { id } => {
            super::crud::replay_status(conn, id, SavingsGoalStatus::Archived)
        }
        SavingsGoalCommand::Unarchive { id } => {
            super::crud::replay_status(conn, id, SavingsGoalStatus::Active)
        }
        SavingsGoalCommand::Delete { id } => super::crud::replay_delete(conn, id),
    }
}
