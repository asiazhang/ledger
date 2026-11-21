use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemCommandRow {
    pub name: String,
    pub purchase_date: String,
    pub total_cost_cents: i64,
    pub currency_code: String,
    pub cost_native_cents: i64,
    pub purchase_transaction_id: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ItemCommand {
    Create {
        id: String,
        row: ItemCommandRow,
    },
    Update {
        id: String,
        row: ItemCommandRow,
    },
    Dispose {
        id: String,
        disposal_date: String,
        residual_value_cents: Option<i64>,
    },
    Delete {
        id: String,
    },
}

impl ItemCommand {
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            ItemCommand::Create { id, .. }
            | ItemCommand::Update { id, .. }
            | ItemCommand::Dispose { id, .. }
            | ItemCommand::Delete { id } => id,
        }
    }
}

impl SyncCommand for ItemCommand {
    const ENTITY: &'static str = "item";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

pub(crate) fn record_local(conn: &Connection, command: ItemCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_command(conn: &Connection, command: &ItemCommand) -> Result<()> {
    match command {
        ItemCommand::Create { id, row } => super::domain::replay_create(conn, id, row),
        ItemCommand::Update { id, row } => super::domain::replay_update(conn, id, row),
        ItemCommand::Dispose {
            id,
            disposal_date,
            residual_value_cents,
        } => super::domain::replay_dispose(
            conn,
            id,
            &super::model::ItemDisposeInput {
                disposal_date: disposal_date.clone(),
                residual_value_cents: *residual_value_cents,
            },
        ),
        ItemCommand::Delete { id } => super::domain::replay_delete(conn, id),
    }
}
