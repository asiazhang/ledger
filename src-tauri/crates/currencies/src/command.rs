use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LedgerSettingCommand {
    SetBaseCurrency { code: String },
}

impl LedgerSettingCommand {
    pub fn subject_id(&self) -> &'static str {
        match self {
            LedgerSettingCommand::SetBaseCurrency { .. } => "base_currency",
        }
    }
}

impl SyncCommand for LedgerSettingCommand {
    const ENTITY: &'static str = "ledger_setting";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}

pub(crate) fn record_local(conn: &Connection, command: LedgerSettingCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_command(conn: &Connection, command: &LedgerSettingCommand) -> Result<()> {
    match command {
        LedgerSettingCommand::SetBaseCurrency { code } => {
            super::base_currency::replay_set_base_currency(conn, code)
        }
    }
}
