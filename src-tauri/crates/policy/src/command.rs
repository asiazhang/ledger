//! 保险域同步命令（issue #860 / ADR-0091）：op 载荷的保单与保司字典形态、产出
//! 单点与重放分派。
//!
//! - **载荷形态**：保单创建/编辑携带解决后的语义行（校验归一化结果，与落库值
//!   一致）；保司创建携带实体 id + 落定名；删除只需实体 id。保单现金流入无独立
//!   实体——流入是挂单（`policy_id`）的收入流水，随交易命令同步。**只增不改**。
//! - **产出单点**（`record_local`）：保单与保司的写编排入口成功后调用，op 随写
//!   事务提交/回滚（保单表单即建保司随建档写事务）。
//! - **重放执行**：与本地写同一执行协议（保司在用校验、保额成对、名字唯一等
//!   原样生效，依赖缺失或唯一冲突挂起待裁决），不产出 op。

use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

use super::model::PolicyInput;
use super::validation::NormalizedInput;

// ---------------------------------------------------------------------------
// 保单（Policy）
// ---------------------------------------------------------------------------

/// 保单命令行载荷（语义字段 = 校验归一化结果；簿记戳不随行携带）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyCommandRow {
    pub insurer_id: String,
    pub policy_number: String,
    pub product_name: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub coverage_amount_cents: Option<i64>,
    pub coverage_currency_code: Option<String>,
    pub note: Option<String>,
}

/// 保单同步命令（serde：`action` 判别）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PolicyCommand {
    /// 创建保单：实体 id 与语义行随命令携带（重放端不得重新生成 id）。
    Create { id: String, row: PolicyCommandRow },
    /// 编辑保单静态要素（全量替换语义，与本地编辑同语义）。
    Update { id: String, row: PolicyCommandRow },
    /// 删除保单（软删除，历史流水引用保留不置空）。
    Delete { id: String },
}

impl PolicyCommand {
    /// 命令指向的实体 id（LWW 裁决域 = 单张保单）。
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            PolicyCommand::Create { id, .. }
            | PolicyCommand::Update { id, .. }
            | PolicyCommand::Delete { id } => id,
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识；op 产出直呼协议面（环依赖由 crate 依赖图断开）。
impl SyncCommand for PolicyCommand {
    const ENTITY: &'static str = "policy";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

impl From<&NormalizedInput> for PolicyCommandRow {
    fn from(n: &NormalizedInput) -> Self {
        Self {
            insurer_id: n.insurer_id.clone(),
            policy_number: n.policy_number.clone(),
            product_name: n.product_name.clone(),
            start_date: n.start_date.clone(),
            end_date: n.end_date.clone(),
            coverage_amount_cents: n.coverage_amount_cents,
            coverage_currency_code: n.coverage_currency_code.clone(),
            note: n.note.clone(),
        }
    }
}

impl From<&PolicyCommandRow> for PolicyInput {
    fn from(row: &PolicyCommandRow) -> Self {
        Self {
            insurer_id: row.insurer_id.clone(),
            policy_number: row.policy_number.clone(),
            product_name: row.product_name.clone(),
            start_date: row.start_date.clone(),
            end_date: row.end_date.clone(),
            coverage_amount_cents: row.coverage_amount_cents,
            coverage_currency_code: row.coverage_currency_code.clone(),
            note: row.note.clone(),
        }
    }
}

/// op 产出接缝（保单集中单点）：本地保单写成功后追加一条 op 进本机 OpLog。
pub(crate) fn record_policy_local(conn: &Connection, command: PolicyCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：与本地写同一执行协议。crate 拆分后跨 crate
/// 消费：sync_engine::registry 经根包再导出面分派（#1100，pub(crate)→pub，
/// 签名与语义不变，#1092 replay_command 同款）。
pub fn replay_policy_command(conn: &Connection, command: &PolicyCommand) -> Result<()> {
    match command {
        PolicyCommand::Create { id, row } => super::crud::replay_create(conn, id, row),
        PolicyCommand::Update { id, row } => super::crud::replay_update(conn, id, row),
        PolicyCommand::Delete { id } => super::crud::replay_delete(conn, id),
    }
}

// ---------------------------------------------------------------------------
// 保司字典（Insurer，保险域自有独立字典，ADR-0082）
// ---------------------------------------------------------------------------

/// 保司同步命令（serde：`action` 判别）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum InsurerCommand {
    /// 创建保司：实体 id 与落定名随命令携带（重放端不得重新生成 id）。
    Create { id: String, name: String },
    /// 修改保司（落定名，trim 后非空）。
    Update { id: String, name: String },
    /// 删除保司（软删除，存量保单引用保留照常显示）。
    Delete { id: String },
}

impl InsurerCommand {
    /// 命令指向的实体 id（LWW 裁决域 = 单个保司）。
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            InsurerCommand::Create { id, .. }
            | InsurerCommand::Update { id, .. }
            | InsurerCommand::Delete { id } => id,
        }
    }
}

/// 协议面契约（#1089）：实体标签与 serde 信封 tag 同源（门 a 二源断言之锚），
/// 实体键派生是域自身知识；op 产出直呼协议面（环依赖由 crate 依赖图断开）。
impl SyncCommand for InsurerCommand {
    const ENTITY: &'static str = "insurer";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

/// op 产出接缝（保司集中单点）：本地保司写成功后追加一条 op 进本机 OpLog。
pub(crate) fn record_insurer_local(conn: &Connection, command: InsurerCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：与本地写同一执行协议。crate 拆分后跨 crate
/// 消费：sync_engine::registry 经根包再导出面分派（#1100，pub(crate)→pub，
/// 签名与语义不变，#1092 replay_command 同款）。
pub fn replay_insurer_command(conn: &Connection, command: &InsurerCommand) -> Result<()> {
    match command {
        InsurerCommand::Create { id, name } => super::insurer::replay_create(conn, id, name),
        InsurerCommand::Update { id, name } => super::insurer::replay_update(conn, id, name),
        InsurerCommand::Delete { id } => super::insurer::replay_delete(conn, id),
    }
}
