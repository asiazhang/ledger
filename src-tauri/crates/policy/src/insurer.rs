use rusqlite::{Connection, OptionalExtension};

use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;

use super::command::{InsurerCommand, record_insurer_local};

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Insurer {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct InsurerInput {
    pub name: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct InsurerUpdateInput {
    pub name: Option<String>,
}

impl FromRow for Insurer {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Insurer {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            version: row.get(4)?,
            device_id: row.get(5)?,
            is_deleted: row.get::<_, i64>(6)? != 0,
        })
    }
}

const INSURER_COLUMNS: &str = "id,name,created_at,updated_at,version,device_id,is_deleted";

pub fn list_insurers(conn: &Connection, include_deleted: bool) -> Result<Vec<Insurer>> {
    let where_clause = if include_deleted {
        ""
    } else {
        "WHERE is_deleted=0"
    };
    query_all(
        conn,
        &format!("SELECT {INSURER_COLUMNS} FROM insurers {where_clause} ORDER BY name, created_at"),
        [],
    )
}

pub fn create_insurer(conn: &Connection, input: InsurerInput) -> Result<String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::coded("insurer.name-required", "保司名不能为空"));
    }
    ensure_transaction(conn, || {
        let id = write_create(conn, &new_uuid(), name)?;
        record_insurer_local(
            conn,
            InsurerCommand::Create {
                id: id.clone(),
                name: name.to_string(),
            },
        )?;
        Ok(id)
    })
}

fn write_create(conn: &Connection, id: &str, name: &str) -> Result<String> {
    if insurer_name_taken(conn, name, None)? {
        return Err(AppError::codedp(
            "insurer.already-exists",
            format!("保司已存在: {name}"),
            &[name],
        ));
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO insurers (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,0)",
        rusqlite::params![id, name, now, now, 1, device_id(conn)?],
    )?;
    Ok(id.to_string())
}

pub fn update_insurer(conn: &Connection, id: &str, input: InsurerUpdateInput) -> Result<()> {
    ensure_transaction(conn, || {
        let name = write_update(conn, id, &input)?;
        record_insurer_local(
            conn,
            InsurerCommand::Update {
                id: id.to_string(),
                name,
            },
        )
    })
}

fn write_update(conn: &Connection, id: &str, input: &InsurerUpdateInput) -> Result<String> {
    let existing: Insurer = query_all(
        conn,
        &format!("SELECT {INSURER_COLUMNS} FROM insurers WHERE id=?1 AND is_deleted=0"),
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| {
        AppError::codedp_not_found("insurer.not-found", format!("保司不存在: {id}"), &[id])
    })?;

    let name = input.name.clone().unwrap_or(existing.name);
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::coded("insurer.name-required", "保司名不能为空"));
    }
    if insurer_name_taken(conn, name, Some(id))? {
        return Err(AppError::codedp(
            "insurer.already-exists",
            format!("保司已存在: {name}"),
            &[name],
        ));
    }

    conn.execute(
        "UPDATE insurers SET name=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
        rusqlite::params![name, now_iso(), device_id(conn)?, id],
    )?;
    Ok(name.to_string())
}

pub fn delete_insurer(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_insurer_local(conn, InsurerCommand::Delete { id: id.to_string() })
    })
}

fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM insurers WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::codedp_not_found(
            "insurer.not-found",
            format!("保司不存在: {id}"),
            &[id],
        ));
    }
    conn.execute(
        "UPDATE insurers SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

pub fn find_insurer_by_name(conn: &Connection, name: &str) -> Result<Option<String>> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM insurers WHERE name=?1 AND is_deleted=0",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id)
}

pub fn create_insurer_by_name(conn: &Connection, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::coded("insurer.name-required", "保司名不能为空"));
    }
    if let Some(id) = find_insurer_by_name(conn, name)? {
        return Ok(id);
    }
    create_insurer(
        conn,
        InsurerInput {
            name: name.to_string(),
        },
    )
}

fn insurer_name_taken(conn: &Connection, name: &str, exclude_id: Option<&str>) -> Result<bool> {
    let found: bool = conn
        .query_row(
            "SELECT 1 FROM insurers WHERE name=?1 AND is_deleted=0 AND (?2 IS NULL OR id<>?2)",
            rusqlite::params![name, exclude_id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    Ok(found)
}

pub(crate) fn replay_create(conn: &Connection, id: &str, name: &str) -> Result<()> {
    write_create(conn, id, name)?;
    Ok(())
}

pub(crate) fn replay_update(conn: &Connection, id: &str, name: &str) -> Result<()> {
    write_update(
        conn,
        id,
        &InsurerUpdateInput {
            name: Some(name.to_string()),
        },
    )?;
    Ok(())
}

pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    write_delete(conn, id)
}
