use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

use super::model::ReorderItem;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryCommandRow {
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CategoryCommand {
    Create {
        id: String,
        row: CategoryCommandRow,
    },
    Update {
        id: String,
        name: String,
        icon: Option<String>,
        parent_id: Option<String>,
    },
    Delete {
        id: String,
    },
    Reorder {
        items: Vec<ReorderItem>,
    },
}

impl CategoryCommand {
    pub(crate) fn subject(&self) -> Option<&str> {
        match self {
            CategoryCommand::Create { id, .. }
            | CategoryCommand::Update { id, .. }
            | CategoryCommand::Delete { id } => Some(id),
            CategoryCommand::Reorder { .. } => None,
        }
    }
}

impl SyncCommand for CategoryCommand {
    const ENTITY: &'static str = "category";

    fn subject(&self) -> Option<Cow<'_, str>> {
        self.subject().map(Cow::Borrowed)
    }
}

pub(crate) fn record_local(conn: &Connection, command: CategoryCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_command(conn: &Connection, command: &CategoryCommand) -> Result<()> {
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
