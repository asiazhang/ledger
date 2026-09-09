//! 跨端语义命令信封（DomainCommand，ADR-0091 决策 2）：op 的载荷形态。
//!
//! 与既有写入接缝同语义的域级命令，按实体分派；重放端经同一编排协议执行，
//! 不绕过不变量。**只增不改**：新增实体/字段只追加（旧日志可在新 schema 上
//! 重放），与已发布契约纪律同构；`entity` tag 与 `sync_ops.entity` 列同源。

use serde::{Deserialize, Serialize};

use crate::currencies::LedgerSettingCommand;
use crate::scheduled_transactions::ScheduledCommand;
use crate::transaction::TransactionCommand;

/// 语义命令（按实体分派；`entity` tag 与 `sync_ops.entity` 列同源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entity", content = "payload", rename_all = "snake_case")]
pub enum DomainCommand {
    /// 交易创建/修改/删除命令（核心交易域）。
    Transaction(TransactionCommand),
    /// 定时计划域命令（期次触发等；ADR-0091 决策 5）。
    Scheduled(ScheduledCommand),
    /// 账本级设置命令（LedgerLevelSetting，issue #858 / ADR-0091 决策 3）。
    LedgerSetting(LedgerSettingCommand),
}

impl DomainCommand {
    /// 实体判别键（`sync_ops.entity` 列取值；与 serde tag 同源，防双源漂移）。
    pub fn entity(&self) -> &'static str {
        match self {
            DomainCommand::Transaction(_) => "transaction",
            DomainCommand::Scheduled(_) => "scheduled",
            DomainCommand::LedgerSetting(_) => "ledger_setting",
        }
    }

    /// 命令指向的实体（实体判别键，实体 id）：LWW 裁决域（同实体并发编辑取
    /// 全序末者，ADR-0091 决策 4）。无实体指向的命令返回 None——其冲突域另行
    /// 裁定（如期次触发命令的 OccurrenceKey，ADR-0091 决策 5）。
    pub fn subject(&self) -> Option<(&'static str, &str)> {
        match self {
            DomainCommand::Transaction(cmd) => Some(("transaction", cmd.subject_id())),
            DomainCommand::Scheduled(_) => None,
            DomainCommand::LedgerSetting(cmd) => Some(("ledger_setting", cmd.subject_id())),
        }
    }
}
