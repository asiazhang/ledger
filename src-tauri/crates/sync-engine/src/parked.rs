use rusqlite::params;

use ledger_infra::db::now_iso;
use ledger_infra::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParkedOp {
    pub op_id: String,
    pub device_id: String,
    pub clock: i64,
    pub schema_version: i64,
    pub entity: String,
    pub entity_id: String,
    pub payload: String,
    pub code: String,
    pub params: Vec<String>,
    pub message: String,
    pub parked_at: String,
}

pub(super) const CODE_SCHEMA_AHEAD: &str = "sync-engine.schema-ahead";
pub(super) const CODE_UNDECODABLE: &str = "sync-engine.op-undecodable";
pub(super) const CODE_REPLAY_FAILED: &str = "sync-engine.replay-failed";

pub(super) fn park(conn: &rusqlite::Connection, op: &ParkedOp) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_parked_ops \
         (op_id, device_id, clock, schema_version, entity, entity_id, payload, park_code, park_params, park_message, parked_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
         ON CONFLICT(op_id) DO UPDATE SET \
           park_code = excluded.park_code, park_params = excluded.park_params, \
           park_message = excluded.park_message, parked_at = excluded.parked_at",
        params![
            op.op_id,
            op.device_id,
            op.clock,
            op.schema_version,
            op.entity,
            op.entity_id,
            op.payload,
            op.code,
            encode_params(&op.params),
            op.message,
            now_iso(),
        ],
    )?;
    Ok(())
}

pub(super) fn resolve(conn: &rusqlite::Connection, op_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM sync_parked_ops WHERE op_id = ?1",
        params![op_id],
    )?;
    Ok(())
}

pub(super) fn is_empty(conn: &rusqlite::Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sync_parked_ops", [], |r| r.get(0))?;
    Ok(count == 0)
}

pub(super) fn list(conn: &rusqlite::Connection) -> Result<Vec<ParkedOp>> {
    let mut stmt = conn.prepare(
        "SELECT op_id, device_id, clock, schema_version, entity, entity_id, payload, \
         park_code, park_params, park_message, parked_at \
         FROM sync_parked_ops ORDER BY clock ASC, device_id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ParkedOp {
            op_id: r.get(0)?,
            device_id: r.get(1)?,
            clock: r.get(2)?,
            schema_version: r.get(3)?,
            entity: r.get(4)?,
            entity_id: r.get(5)?,
            payload: r.get(6)?,
            code: r.get(7)?,
            params: decode_params(r.get::<_, String>(8)?),
            message: r.get(9)?,
            parked_at: r.get(10)?,
        })
    })?;
    let mut ops = Vec::new();
    for row in rows {
        ops.push(row?);
    }
    Ok(ops)
}

fn encode_params(params: &[String]) -> String {
    serde_json::to_string(params).unwrap_or_else(|_| "[]".to_string())
}

fn decode_params(raw: String) -> Vec<String> {
    serde_json::from_str(&raw).unwrap_or_default()
}
