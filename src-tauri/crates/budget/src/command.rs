use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::BudgetPeriod;
use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BudgetCommand {
    Create {
        id: String,
        category_id: String,
        period: BudgetPeriod,
        amount_cents: i64,
        start_date: String,
    },
    Update {
        id: String,
        amount_cents: i64,
        #[serde(default)]
        period: Option<BudgetPeriod>,
    },
    Delete {
        id: String,
    },
}

impl BudgetCommand {
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            BudgetCommand::Create { id, .. }
            | BudgetCommand::Update { id, .. }
            | BudgetCommand::Delete { id } => id,
        }
    }
}

impl SyncCommand for BudgetCommand {
    const ENTITY: &'static str = "budget";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

pub(crate) fn record_local(conn: &Connection, command: BudgetCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_command(conn: &Connection, command: &BudgetCommand) -> Result<()> {
    match command {
        BudgetCommand::Create {
            id,
            category_id,
            period,
            amount_cents,
            start_date,
        } => super::crud::replay_create(conn, id, category_id, *period, *amount_cents, start_date),
        BudgetCommand::Update {
            id,
            amount_cents,
            period,
        } => super::crud::replay_update(conn, id, *amount_cents, *period),
        BudgetCommand::Delete { id } => super::crud::write_delete(conn, id),
    }
}
