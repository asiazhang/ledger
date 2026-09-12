//! 账本级设置同步命令（issue #858 / ADR-0091 决策 3/9）：LedgerLevelSetting 的
//! op 载荷形态、产出单点与重放分派。
//!
//! - **载荷形态**（[`LedgerSettingCommand`]）：账本级设置的语义命令——设置项
//!   标识即命令变体，值随行携带；LWW 裁决域是单个设置项（同设置项并发修改取
//!   全序末者，见 [`LedgerSettingCommand::subject_id`]）。**只增不改**：新设置
//!   项只追加变体，旧日志可在新 schema 重放。
//! - **产出单点**（[`record_local`]）：本域写编排入口（`base_currency::
//!   set_base_currency`）成功后调用，op 随写事务提交/回滚。
//! - **重放执行**（[`replay_command`]）：与本地写同一执行协议（校验 + 落库），
//!   不产出 op——外来 op 由同步引擎落日志。

use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

/// 账本级设置命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LedgerSettingCommand {
    /// 设置本位币基准（LedgerLevelSetting 首个成员；值 = 币种代码，折算口径
    /// 随之全设备一致，ADR-0091 决策 3）。
    SetBaseCurrency { code: String },
}

impl LedgerSettingCommand {
    /// 设置项标识（实体判别键的 id 半边）：LWW 按设置项逐项裁决——并发修改
    /// 同一设置项取全序末者，不同设置项互不压制。
    pub fn subject_id(&self) -> &'static str {
        match self {
            LedgerSettingCommand::SetBaseCurrency { .. } => "base_currency",
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识（裁决域 = 单个设置项）；op 产出直呼协议面
///（环依赖由 crate 依赖图断开）。
impl SyncCommand for LedgerSettingCommand {
    const ENTITY: &'static str = "ledger_setting";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

/// op 产出接缝（账本级设置集中单点）：本地设置写成功后追加一条 op 进本机
/// OpLog。仅本域写编排入口调用；随写事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: LedgerSettingCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：按设置项转发到对应执行协议。
pub(crate) fn replay_command(conn: &Connection, command: &LedgerSettingCommand) -> Result<()> {
    match command {
        LedgerSettingCommand::SetBaseCurrency { code } => {
            super::base_currency::replay_set_base_currency(conn, code)
        }
    }
}
