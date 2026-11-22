use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

use super::model::AccountType;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountCommandRow {
    pub name: String,
    pub kind: AccountType,
    pub currency_code: String,
    pub initial_balance_cents: i64,
    pub is_hidden: bool,
    pub credit_limit_cents: Option<i64>,
    pub statement_day: Option<i64>,
    pub due_day: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AccountCommand {
    Create {
        id: String,
        row: AccountCommandRow,
    },
    Update {
        id: String,
        name: String,
        currency_code: String,
        credit_limit_cents: Option<i64>,
        statement_day: Option<i64>,
        due_day: Option<i64>,
    },
    Delete {
        id: String,
    },
    SetArchived {
        id: String,
        archived: bool,
    },
}

impl AccountCommand {
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            AccountCommand::Create { id, .. }
            | AccountCommand::Update { id, .. }
            | AccountCommand::Delete { id }
            | AccountCommand::SetArchived { id, .. } => id,
        }
    }
}

impl SyncCommand for AccountCommand {
    const ENTITY: &'static str = "account";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

pub(crate) fn record_local(conn: &Connection, command: AccountCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_command(conn: &Connection, command: &AccountCommand) -> Result<()> {
    match command {
        AccountCommand::Create { id, row } => super::core::replay_create(conn, id, row),
        AccountCommand::Update {
            id,
            name,
            currency_code,
            credit_limit_cents,
            statement_day,
            due_day,
        } => super::core::replay_update(
            conn,
            id,
            name,
            currency_code,
            *credit_limit_cents,
            *statement_day,
            *due_day,
        ),
        AccountCommand::Delete { id } => super::core::replay_delete(conn, id),
        AccountCommand::SetArchived { id, archived } => {
            super::core::replay_set_archived(conn, id, *archived)
        }
    }
}
