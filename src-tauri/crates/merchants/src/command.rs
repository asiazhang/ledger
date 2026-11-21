use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MerchantCommand {
    Create { id: String, name: String },
    Update { id: String, name: String },
    Delete { id: String },
}

impl MerchantCommand {
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            MerchantCommand::Create { id, .. }
            | MerchantCommand::Update { id, .. }
            | MerchantCommand::Delete { id } => id,
        }
    }
}

impl SyncCommand for MerchantCommand {
    const ENTITY: &'static str = "merchant";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

pub(crate) fn record_local(conn: &Connection, command: MerchantCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_command(conn: &Connection, command: &MerchantCommand) -> Result<()> {
    match command {
        MerchantCommand::Create { id, name } => super::crud::replay_create(conn, id, name),
        MerchantCommand::Update { id, name } => super::crud::replay_update(conn, id, name),
        MerchantCommand::Delete { id } => super::crud::replay_delete(conn, id),
    }
}
