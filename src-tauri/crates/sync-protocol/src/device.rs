use rusqlite::{Connection, OptionalExtension, params};

use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::Result;

pub fn device_id(conn: &Connection) -> Result<String> {
    let existing: Option<String> = conn
        .query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO sync_device (id, logical_clock, created_at, updated_at) VALUES (?1, 0, ?2, ?2)",
        params![id, now],
    )?;
    tracing::debug!(device_id = %id, "首次使用：已生成本机设备标识");
    Ok(id)
}

pub fn next_clock(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "UPDATE sync_device SET logical_clock = logical_clock + 1, updated_at = ?1 \
         RETURNING logical_clock",
        params![now_iso()],
        |r| r.get(0),
    )
    .map_err(Into::into)
}
