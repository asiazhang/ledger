//! 商户同步命令（issue #860 / ADR-0091）：op 载荷的商户域形态、产出单点与重放分派。
//!
//! - **载荷形态**（[`MerchantCommand`]）：创建携带实体 id + 名称；修改携带解决后
//!   的落定名（trim 后非空）；删除只需实体 id。商户即建路径（交易 plan 的
//!   find-or-create）命中复用不产出 op，未命中即建走创建协议同产 op。
//!   **只增不改**。
//! - **产出单点**（[`record_local`]）：商户写编排入口（创建 / 修改 / 删除）成功后
//!   调用，op 随写事务提交/回滚（交易 plan 内即建随交易写事务）。
//! - **重放执行**（[`replay_command`]）：与本地写同一执行协议（名字唯一校验原样
//!   生效，双端离线各建同名商户后到者在重放端挂起待裁决），不产出 op。

use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

/// 商户同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MerchantCommand {
    /// 创建商户：实体 id 与名称随命令携带（重放端不得重新生成 id）。
    Create { id: String, name: String },
    /// 修改商户（落定名，trim 后非空）。
    Update { id: String, name: String },
    /// 删除商户（软删除，历史交易引用保留）。
    Delete { id: String },
}

impl MerchantCommand {
    /// 命令指向的实体 id（LWW 裁决域 = 单个商户）。
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            MerchantCommand::Create { id, .. }
            | MerchantCommand::Update { id, .. }
            | MerchantCommand::Delete { id } => id,
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识；op 产出直呼协议面（环依赖由 crate 依赖图断开）。
impl SyncCommand for MerchantCommand {
    const ENTITY: &'static str = "merchant";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

/// op 产出接缝（商户域集中单点）：本地商户写成功后追加一条 op 进本机 OpLog。
///
/// 仅商户写编排入口（`merchants::crud` 的创建 / 修改 / 删除协议）调用；随编排
/// 事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: MerchantCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：按动作转发到与本地写同一执行协议。
/// #1096 拆 crate 起 `pub(crate)`→`pub`（sync_engine 经根包再导出面消费，跨 crate
/// 可见性要求，签名与语义不变）。
pub fn replay_command(conn: &Connection, command: &MerchantCommand) -> Result<()> {
    match command {
        MerchantCommand::Create { id, name } => super::crud::replay_create(conn, id, name),
        MerchantCommand::Update { id, name } => super::crud::replay_update(conn, id, name),
        MerchantCommand::Delete { id } => super::crud::replay_delete(conn, id),
    }
}
