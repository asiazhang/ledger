//! 账户同步命令（issue #860 / ADR-0091）：op 载荷的账户域形态、产出单点与重放分派。
//!
//! - **载荷形态**（[`AccountCommand`]）：创建携带实体 id + 语义行（类型、币种、
//!   期初余额、隐藏标志——黑洞账户即建路径同走此命令，`is_hidden` 随行）；修改
//!   携带解决后的语义值（名称非空、币种为落定值，重放端以同一协议复验依赖后
//!   落库）；删除只需实体 id。**只增不改**：字段演进只追加可选成员，旧日志可
//!   在新 schema 重放。
//! - **产出单点**（[`record_local`]）：账户写编排入口（创建 / 修改 / 删除 /
//!   黑洞即建）成功后调用，op 随写事务提交/回滚。
//! - **重放执行**（[`replay_command`]）：与本地写同一执行协议（依赖校验 + 落库），
//!   不产出 op——外来 op 由同步引擎落日志。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::sync_engine::{DomainCommand, record_local as record_op};

use super::model::AccountType;

/// 账户命令行载荷（语义字段）：簿记戳（created_at/updated_at/version/device_id）
/// 是各端本地事实，不随行携带（ADR-0091：op 只携带 DeviceId / 逻辑时钟 / schema 版本）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountCommandRow {
    pub name: String,
    pub kind: AccountType,
    pub currency_code: String,
    pub initial_balance_cents: i64,
    /// 黑洞账户标志：余额调整即建路径携带 `true`，与种子同形（AI 导入域
    /// BlackHoleAccount）；常规创建恒 `false`。
    pub is_hidden: bool,
}

/// 账户同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AccountCommand {
    /// 创建账户：实体 id 与语义行随命令携带（重放端不得重新生成 id）。
    Create { id: String, row: AccountCommandRow },
    /// 修改账户（名称 / 币种为解决后的落定值，与本地修改同语义）。
    Update {
        id: String,
        name: String,
        currency_code: String,
    },
    /// 删除账户（软删除）：实体 id 足够——重放端执行与本地删除同一协议。
    Delete { id: String },
}

impl AccountCommand {
    /// 命令指向的实体 id（LWW 裁决域 = 单个账户）。
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            AccountCommand::Create { id, .. }
            | AccountCommand::Update { id, .. }
            | AccountCommand::Delete { id } => id,
        }
    }
}

/// op 产出接缝（账户域集中单点）：本地账户写成功后追加一条 op 进本机 OpLog。
///
/// 仅账户写编排入口（`accounts::core` 的创建 / 修改 / 删除协议与黑洞即建）调用；
/// 随编排事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: AccountCommand) -> Result<()> {
    record_op(conn, DomainCommand::Account(command))?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：按动作转发到与本地写同一执行协议。
pub(crate) fn replay_command(conn: &Connection, command: &AccountCommand) -> Result<()> {
    match command {
        AccountCommand::Create { id, row } => super::core::replay_create(conn, id, row),
        AccountCommand::Update {
            id,
            name,
            currency_code,
        } => super::core::replay_update(conn, id, name, currency_code),
        AccountCommand::Delete { id } => super::core::replay_delete(conn, id),
    }
}
