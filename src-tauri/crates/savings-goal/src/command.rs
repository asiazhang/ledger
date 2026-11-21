use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

use super::model::SavingsGoalStatus;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SavingsGoalCommand {
    Create {
        id: String,
        account_id: String,
        name: String,
        target_amount_cents: i64,
        deadline: Option<String>,
    },
    Update {
        id: String,
        name: String,
        target_amount_cents: i64,
        deadline: Option<String>,
        planned_monthly_cents: Option<i64>,
    },
    Archive {
        id: String,
    },
    Unarchive {
        id: String,
    },
    Delete {
        id: String,
    },
}

impl SavingsGoalCommand {
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            SavingsGoalCommand::Create { id, .. }
            | SavingsGoalCommand::Update { id, .. }
            | SavingsGoalCommand::Archive { id }
            | SavingsGoalCommand::Unarchive { id }
            | SavingsGoalCommand::Delete { id } => id,
        }
    }
}

impl SyncCommand for SavingsGoalCommand {
    const ENTITY: &'static str = "goal";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

pub(crate) fn record_local(conn: &Connection, command: SavingsGoalCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_command(conn: &Connection, command: &SavingsGoalCommand) -> Result<()> {
    match command {
        SavingsGoalCommand::Create {
            id,
            account_id,
            name,
            target_amount_cents,
            deadline,
        } => super::crud::replay_create(
            conn,
            id,
            account_id,
            name,
            *target_amount_cents,
            deadline.as_deref(),
        ),
        SavingsGoalCommand::Update {
            id,
            name,
            target_amount_cents,
            deadline,
            planned_monthly_cents,
        } => super::crud::replay_update(
            conn,
            id,
            name,
            *target_amount_cents,
            deadline.as_deref(),
            *planned_monthly_cents,
        ),
        SavingsGoalCommand::Archive { id } => {
            super::crud::replay_status(conn, id, SavingsGoalStatus::Archived)
        }
        SavingsGoalCommand::Unarchive { id } => {
            super::crud::replay_status(conn, id, SavingsGoalStatus::Active)
        }
        SavingsGoalCommand::Delete { id } => super::crud::replay_delete(conn, id),
    }
}
