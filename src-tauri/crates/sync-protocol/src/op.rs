use std::sync::OnceLock;

use rusqlite::{Connection, OptionalExtension, params};

use ledger_infra::db::{new_uuid, now_iso, schema_version};
use ledger_infra::error::{AppError, Result};

use super::command::SyncCommand;
use super::device;
use super::position;

static AFTER_WRITE_HOOK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn install_after_write_hook(hook: impl Fn() + Send + Sync + 'static) -> bool {
    AFTER_WRITE_HOOK.set(Box::new(hook)).is_ok()
}

fn notify_after_write() {
    if let Some(hook) = AFTER_WRITE_HOOK.get() {
        hook();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedOp {
    pub op_id: String,
    pub device_id: String,
    pub clock: i64,
    pub schema_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpRow {
    pub op_id: String,
    pub device_id: String,
    pub clock: i64,
    pub schema_version: i64,
    pub entity: String,
    pub entity_id: String,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawOp {
    pub op_id: String,
    pub device_id: String,
    pub clock: i64,
    pub schema_version: i64,
    pub payload: String,
}

pub fn record_local<C: SyncCommand>(conn: &Connection, command: &C) -> Result<RecordedOp> {
    let device_id = device::device_id(conn)?;
    let clock = device::next_clock(conn)?;
    let op_id = new_uuid();
    let schema_version = schema_version(conn)?;
    #[derive(serde::Serialize)]
    struct EnvelopeRef<'a> {
        entity: &'a str,
        payload: serde_json::Value,
    }
    let payload = serde_json::to_value(command)
        .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?;
    let envelope = serde_json::to_string(&EnvelopeRef {
        entity: C::ENTITY,
        payload,
    })
    .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?;
    let row = OpRow {
        op_id: op_id.clone(),
        device_id: device_id.clone(),
        clock,
        schema_version,
        entity: C::ENTITY.to_string(),
        entity_id: command
            .subject()
            .map(|id| id.into_owned())
            .unwrap_or_default(),
        payload: envelope,
    };
    insert_row(conn, &row)?;
    position::advance(conn, &row.device_id, clock)?;
    notify_after_write();
    Ok(RecordedOp {
        op_id,
        device_id,
        clock,
        schema_version,
    })
}

pub fn insert_row(conn: &Connection, row: &OpRow) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_ops (op_id, device_id, clock, schema_version, entity, entity_id, payload, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            row.op_id,
            row.device_id,
            row.clock,
            row.schema_version,
            row.entity,
            row.entity_id,
            row.payload,
            now_iso(),
        ],
    )?;
    Ok(())
}

pub fn has_later_subject(
    conn: &Connection,
    entity: &str,
    entity_id: &str,
    clock: i64,
    device_id: &str,
) -> Result<bool> {
    let later = conn
        .query_row(
            "SELECT 1 FROM sync_ops \
             WHERE entity = ?1 AND entity_id = ?2 \
               AND (clock > ?3 OR (clock = ?3 AND device_id > ?4)) \
             LIMIT 1",
            params![entity, entity_id, clock, device_id],
            |_| Ok(()),
        )
        .optional()?;
    Ok(later.is_some())
}

pub fn is_known(conn: &Connection, op_id: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sync_ops WHERE op_id = ?1",
            params![op_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub fn is_known_at(conn: &Connection, device_id: &str, clock: i64) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sync_ops WHERE device_id = ?1 AND clock = ?2",
            params![device_id, clock],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub fn delete_stream_before(
    conn: &Connection,
    device_id: &str,
    through_position: i64,
) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM sync_ops WHERE device_id = ?1 AND clock <= ?2",
        params![device_id, through_position],
    )?;
    Ok(deleted)
}

pub fn is_empty(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sync_ops", [], |r| r.get(0))?;
    Ok(count == 0)
}

pub fn read_all_raw(conn: &Connection) -> Result<Vec<RawOp>> {
    let mut stmt =
        conn.prepare("SELECT op_id, device_id, clock, schema_version, payload FROM sync_ops")?;
    let rows = stmt.query_map([], map_op_row)?;
    let mut ops = Vec::new();
    for row in rows {
        ops.push(row?);
    }
    Ok(ops)
}

pub fn read_own_since_raw(
    conn: &Connection,
    device_id: &str,
    after_clock: i64,
) -> Result<Vec<RawOp>> {
    let mut stmt = conn.prepare(
        "SELECT op_id, device_id, clock, schema_version, payload FROM sync_ops \
         WHERE device_id = ?1 AND clock > ?2 ORDER BY clock ASC",
    )?;
    let rows = stmt.query_map(params![device_id, after_clock], map_op_row)?;
    let mut ops = Vec::new();
    for row in rows {
        ops.push(row?);
    }
    Ok(ops)
}

fn map_op_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawOp> {
    Ok(RawOp {
        op_id: r.get(0)?,
        device_id: r.get(1)?,
        clock: r.get(2)?,
        schema_version: r.get(3)?,
        payload: r.get(4)?,
    })
}
