use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

use super::model::SyncOp;

pub(crate) fn insert_row(conn: &Connection, op: &SyncOp) -> Result<()> {
    let payload = serde_json::to_string(&op.command)
        .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?;
    let (entity, entity_id) = op.command.subject();
    ledger_sync_protocol::op::insert_row(
        conn,
        &ledger_sync_protocol::op::OpRow {
            op_id: op.op_id.clone(),
            device_id: op.device_id.clone(),
            clock: op.clock,
            schema_version: op.schema_version,
            entity: entity.to_string(),
            entity_id: entity_id.map(|id| id.into_owned()).unwrap_or_default(),
            payload,
        },
    )
}

pub(crate) fn has_later_subject(
    conn: &Connection,
    entity: &str,
    entity_id: &str,
    clock: i64,
    device_id: &str,
) -> Result<bool> {
    ledger_sync_protocol::op::has_later_subject(conn, entity, entity_id, clock, device_id)
}

pub(crate) fn is_known(conn: &Connection, op_id: &str) -> Result<bool> {
    ledger_sync_protocol::op::is_known(conn, op_id)
}

pub(crate) fn delete_stream_before(
    conn: &Connection,
    device_id: &str,
    through_position: i64,
) -> Result<usize> {
    ledger_sync_protocol::op::delete_stream_before(conn, device_id, through_position)
}

pub(crate) fn is_empty(conn: &Connection) -> Result<bool> {
    ledger_sync_protocol::op::is_empty(conn)
}

pub(crate) fn read_all(conn: &Connection) -> Result<Vec<SyncOp>> {
    let mut ops = Vec::new();
    for raw in ledger_sync_protocol::op::read_all_raw(conn)? {
        ops.push(op_from_raw(raw)?);
    }
    Ok(ops)
}

pub(crate) fn read_own_since(
    conn: &Connection,
    device_id: &str,
    after_clock: i64,
) -> Result<Vec<SyncOp>> {
    let mut ops = Vec::new();
    for raw in ledger_sync_protocol::op::read_own_since_raw(conn, device_id, after_clock)? {
        ops.push(op_from_raw(raw)?);
    }
    Ok(ops)
}

fn op_from_raw(raw: ledger_sync_protocol::op::RawOp) -> Result<SyncOp> {
    let command = serde_json::from_str(&raw.payload)
        .map_err(|e| AppError::Invalid(format!("op 载荷反序列化失败: {e}")))?;
    Ok(SyncOp {
        op_id: raw.op_id,
        device_id: raw.device_id,
        clock: raw.clock,
        schema_version: raw.schema_version,
        command,
    })
}
