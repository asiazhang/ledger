use rusqlite::{Connection, OptionalExtension, params};

use ledger_infra::db::now_iso;
use ledger_infra::error::Result;

use super::op;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPosition {
    pub device_id: String,
    pub applied_through: i64,
}

pub fn is_empty(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sync_stream_positions", [], |r| {
        r.get(0)
    })?;
    Ok(count == 0)
}

pub fn list(conn: &Connection) -> Result<Vec<StreamPosition>> {
    let mut stmt = conn.prepare(
        "SELECT device_id, applied_through FROM sync_stream_positions ORDER BY device_id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(StreamPosition {
            device_id: r.get(0)?,
            applied_through: r.get(1)?,
        })
    })?;
    let mut positions = Vec::new();
    for row in rows {
        positions.push(row?);
    }
    Ok(positions)
}

pub fn position_of(conn: &Connection, device_id: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT applied_through FROM sync_stream_positions WHERE device_id = ?1",
            params![device_id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn advance(conn: &Connection, device_id: &str, decided_clock: i64) -> Result<()> {
    let position = match position_of(conn, device_id)? {
        None => {
            conn.execute(
                "INSERT INTO sync_stream_positions (device_id, applied_through, updated_at) \
                 VALUES (?1, 0, ?2)",
                params![device_id, now_iso()],
            )?;
            0
        }
        Some(position) if decided_clock <= position => return Ok(()),
        Some(position) => position,
    };
    let mut through = position;
    while op::is_known_at(conn, device_id, through + 1)? {
        through += 1;
    }
    if through > position {
        conn.execute(
            "UPDATE sync_stream_positions SET applied_through = ?2, updated_at = ?3 \
             WHERE device_id = ?1",
            params![device_id, through, now_iso()],
        )?;
    }
    Ok(())
}

pub fn replace_all(conn: &Connection, positions: &[StreamPosition]) -> Result<()> {
    conn.execute("DELETE FROM sync_stream_positions", [])?;
    for position in positions {
        conn.execute(
            "INSERT INTO sync_stream_positions (device_id, applied_through, updated_at) \
             VALUES (?1, ?2, ?3)",
            params![position.device_id, position.applied_through, now_iso()],
        )?;
    }
    Ok(())
}
