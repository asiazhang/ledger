use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

use super::model::PolicyInput;
use super::validation::NormalizedInput;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PolicyCommand {
    Create { id: String, row: PolicyCommandRow },
    Update { id: String, row: PolicyCommandRow },
    Delete { id: String },
}

impl PolicyCommand {
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            PolicyCommand::Create { id, .. }
            | PolicyCommand::Update { id, .. }
            | PolicyCommand::Delete { id } => id,
        }
    }
}

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

pub(crate) fn record_policy_local(conn: &Connection, command: PolicyCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_policy_command(conn: &Connection, command: &PolicyCommand) -> Result<()> {
    match command {
        PolicyCommand::Create { id, row } => super::crud::replay_create(conn, id, row),
        PolicyCommand::Update { id, row } => super::crud::replay_update(conn, id, row),
        PolicyCommand::Delete { id } => super::crud::replay_delete(conn, id),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum InsurerCommand {
    Create { id: String, name: String },
    Update { id: String, name: String },
    Delete { id: String },
}

impl InsurerCommand {
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            InsurerCommand::Create { id, .. }
            | InsurerCommand::Update { id, .. }
            | InsurerCommand::Delete { id } => id,
        }
    }
}

impl SyncCommand for InsurerCommand {
    const ENTITY: &'static str = "insurer";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

pub(crate) fn record_insurer_local(conn: &Connection, command: InsurerCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_insurer_command(conn: &Connection, command: &InsurerCommand) -> Result<()> {
    match command {
        InsurerCommand::Create { id, name } => super::insurer::replay_create(conn, id, name),
        InsurerCommand::Update { id, name } => super::insurer::replay_update(conn, id, name),
        InsurerCommand::Delete { id } => super::insurer::replay_delete(conn, id),
    }
}
