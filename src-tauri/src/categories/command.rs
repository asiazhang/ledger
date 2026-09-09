//! 分类同步命令（issue #860 / ADR-0091）：op 载荷的分类域形态、产出单点与重放分派。
//!
//! - **载荷形态**（[`CategoryCommand`]）：创建携带实体 id + 语义行；修改携带解决后
//!   的语义值（名称/图标/父分类为落定值）；删除只需实体 id；排序重排整批随行
//!   （重排是全集覆盖语义，无单一实体指向、不参与同实体 LWW）。**只增不改**。
//! - **产出单点**（[`record_local`]）：分类写编排入口（创建 / 修改 / 删除 / 重排）
//!   成功后调用，op 随写事务提交/回滚。
//! - **重放执行**（[`replay_command`]）：与本地写同一执行协议（两级分类校验、
//!   预算删除守卫等依赖检查原样生效，依赖缺失即挂起），不产出 op。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::sync_engine::{DomainCommand, record_local as record_op};

use super::model::ReorderItem;

/// 分类命令行载荷（语义字段；簿记戳不随行携带）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryCommandRow {
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
}

/// 分类同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CategoryCommand {
    /// 创建分类：实体 id 与语义行随命令携带（重放端不得重新生成 id）。
    Create { id: String, row: CategoryCommandRow },
    /// 修改分类（名称 / 图标 / 父分类为解决后的落定值，与本地修改同语义）。
    Update {
        id: String,
        name: String,
        icon: Option<String>,
        parent_id: Option<String>,
    },
    /// 删除分类（软删除，重放端执行同一协议含预算删除守卫）。
    Delete { id: String },
    /// 排序重排（整批落定值随行）：全集覆盖语义，无单一实体指向（不参与
    /// 同实体 LWW，各端按全序执行末次重排自然收敛）。
    Reorder { items: Vec<ReorderItem> },
}

impl CategoryCommand {
    /// 命令指向的实体 id（LWW 裁决域 = 单个分类）；重排无实体指向。
    pub(crate) fn subject(&self) -> Option<(&'static str, &str)> {
        match self {
            CategoryCommand::Create { id, .. }
            | CategoryCommand::Update { id, .. }
            | CategoryCommand::Delete { id } => Some(("category", id)),
            CategoryCommand::Reorder { .. } => None,
        }
    }
}

/// op 产出接缝（分类域集中单点）：本地分类写成功后追加一条 op 进本机 OpLog。
///
/// 仅分类写编排入口（`categories::core` 的创建 / 修改 / 删除 / 重排协议）调用；
/// 随编排事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: CategoryCommand) -> Result<()> {
    record_op(conn, DomainCommand::Category(command))?;
    Ok(())
}

/// 重放执行（同步引擎分派接缝）：按动作转发到与本地写同一执行协议。
pub(crate) fn replay_command(conn: &Connection, command: &CategoryCommand) -> Result<()> {
    match command {
        CategoryCommand::Create { id, row } => super::core::replay_create(conn, id, row),
        CategoryCommand::Update {
            id,
            name,
            icon,
            parent_id,
        } => super::core::replay_update(conn, id, name, icon.as_deref(), parent_id.as_deref()),
        CategoryCommand::Delete { id } => super::core::replay_delete(conn, id),
        CategoryCommand::Reorder { items } => super::core::write_reorder(conn, items),
    }
}
