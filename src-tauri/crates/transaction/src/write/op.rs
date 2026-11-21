use rusqlite::Connection;

use ledger_infra::error::Result;
use ledger_sync_protocol::op::record_local as record_op;

use crate::command::TransactionCommand;

pub(crate) fn record_local(conn: &Connection, command: TransactionCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}
