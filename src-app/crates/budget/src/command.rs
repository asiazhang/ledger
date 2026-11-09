//! 预算同步命令（issue #860 / ADR-0091）：op 载荷的预算域形态、产出单点与重放分派。
//!
//! - **载荷形态**（[`BudgetCommand`]）：创建携带实体 id + 语义行（分类 / 周期 /
//!   金额 / `start_date` 冻结残留随行保真）；编辑携带落定金额与目标周期（`period`
//!   serde default 缺省 = 不改周期，存量 OpLog 旧载荷反序列化语义不变；旧版本设备
//!   重放本载荷时按 serde 默认丢弃未知字段，周期维持旧值，issue #1946）；删除只需
//!   实体 id。**只增不改**。
//! - **产出单点**（[`record_local`]）：预算写编排入口（创建 / 编辑 / 删除）成功后
//!   调用，op 随写事务提交/回滚。
//! - **重放执行**（[`replay_command`]）：与本地写同一执行协议（金额 / 支出分类 /
//!   「分类 + 周期」唯一校验原样生效，双端离线各建同分类同周期预算后到者在重放
//!   端挂起待裁决），不产出 op。

use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::BudgetPeriod;
use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

/// 预算同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BudgetCommand {
    /// 创建预算：实体 id 与语义行随命令携带（重放端不得重新生成 id）。
    Create {
        id: String,
        category_id: String,
        period: BudgetPeriod,
        amount_cents: i64,
        /// 冻结残留（ADR-0029）：不参与计算，随行保真（各端行内容一致）。
        start_date: String,
    },
    /// 编辑预算金额与周期（issue #184 只许金额 → #1946 放开周期，ADR-0029 修订注记；
    /// 分类不可改，改法为删旧建新）。`period` serde default 缺省 = 不改周期。
    Update {
        id: String,
        amount_cents: i64,
        #[serde(default)]
        period: Option<BudgetPeriod>,
    },
    /// 删除预算（软删除；与本地删除同语义：id 不存在时静默无效果）。
    Delete { id: String },
}

impl BudgetCommand {
    /// 命令指向的实体 id（LWW 裁决域 = 单条预算）。
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            BudgetCommand::Create { id, .. }
            | BudgetCommand::Update { id, .. }
            | BudgetCommand::Delete { id } => id,
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识；op 产出直呼协议面（环依赖由 crate 依赖图断开）。
impl SyncCommand for BudgetCommand {
    const ENTITY: &'static str = "budget";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

/// op 产出接缝（预算域集中单点）：本地预算写成功后追加一条 op 进本机 OpLog。
///
/// 仅预算写编排入口（`budget::crud` 的创建 / 编辑 / 删除协议）调用；随编排事务
/// 提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: BudgetCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：按动作转发到与本地写同一执行协议。
/// crate 拆分后跨 crate 消费：sync_engine::registry 经根包再导出面分派（#1101，
/// pub(crate)→pub，签名与语义不变，#1092 replay_command 同款）。
pub fn replay_command(conn: &Connection, command: &BudgetCommand) -> Result<()> {
    match command {
        BudgetCommand::Create {
            id,
            category_id,
            period,
            amount_cents,
            start_date,
        } => super::crud::replay_create(conn, id, category_id, *period, *amount_cents, start_date),
        BudgetCommand::Update {
            id,
            amount_cents,
            period,
        } => super::crud::replay_update(conn, id, *amount_cents, *period),
        BudgetCommand::Delete { id } => super::crud::write_delete(conn, id),
    }
}
