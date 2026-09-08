//! 跨端语义命令信封（DomainCommand，ADR-0091 决策 2）：op 的载荷形态。
//!
//! 与既有写入接缝同语义的域级命令，按实体分派；重放端经同一编排协议执行，
//! 不绕过不变量。**只增不改**：新增实体/字段只追加（旧日志可在新 schema 上
//! 重放），与已发布契约纪律同构；`entity` tag 与 `sync_ops.entity` 列同源。

use serde::{Deserialize, Serialize};

use crate::transaction::TransactionCommand;

/// 语义命令（按实体分派；`entity` tag 与 `sync_ops.entity` 列同源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entity", content = "payload", rename_all = "snake_case")]
pub enum DomainCommand {
    /// 交易创建/修改/删除命令（核心交易域）。
    Transaction(TransactionCommand),
}

impl DomainCommand {
    /// 实体判别键（`sync_ops.entity` 列取值；与 serde tag 同源，防双源漂移）。
    pub fn entity(&self) -> &'static str {
        match self {
            DomainCommand::Transaction(_) => "transaction",
        }
    }
}
