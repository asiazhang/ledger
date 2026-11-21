use rusqlite::{Connection, OptionalExtension};

use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;

use super::command::{MerchantCommand, record_local};
use super::model::{Merchant, MerchantInput, MerchantTransactionCount, MerchantUpdateInput};

const MERCHANT_COLUMNS: &str = "id,name,created_at,updated_at,version,device_id,is_deleted";

pub fn list_merchants(conn: &Connection, include_deleted: bool) -> Result<Vec<Merchant>> {
    let where_clause = if include_deleted {
        ""
    } else {
        "WHERE is_deleted=0"
    };
    query_all(
        conn,
        &format!(
            "SELECT {MERCHANT_COLUMNS} FROM merchants {where_clause} ORDER BY name, created_at"
        ),
        [],
    )
}

pub fn create_merchant(conn: &Connection, input: MerchantInput) -> Result<String> {
    ensure_transaction(conn, || {
        let id = write_create(conn, &new_uuid(), &input.name)?;
        record_local(
            conn,
            MerchantCommand::Create {
                id: id.clone(),
                name: input.name,
            },
        )?;
        Ok(id)
    })
}

fn write_create(conn: &Connection, id: &str, name: &str) -> Result<String> {
    if merchant_name_taken(conn, name, None)? {
        return Err(AppError::codedp(
            "merchant.already-exists",
            format!("商户已存在: {name}"),
            &[name],
        ));
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,0)",
        rusqlite::params![id, name, now, now, 1, device_id(conn)?],
    )?;
    Ok(id.to_string())
}

pub fn get_merchant(conn: &Connection, id: &str) -> Result<Merchant> {
    query_all(
        conn,
        &format!("SELECT {MERCHANT_COLUMNS} FROM merchants WHERE id=?1 AND is_deleted=0"),
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| {
        AppError::codedp_not_found("merchant.not-found", format!("商户不存在: {id}"), &[id])
    })
}

pub fn update_merchant(conn: &Connection, id: &str, input: MerchantUpdateInput) -> Result<()> {
    ensure_transaction(conn, || {
        let name = write_update(conn, id, &input)?;
        record_local(
            conn,
            MerchantCommand::Update {
                id: id.to_string(),
                name,
            },
        )
    })
}

fn write_update(conn: &Connection, id: &str, input: &MerchantUpdateInput) -> Result<String> {
    let existing = get_merchant(conn, id)?;

    let name = match input.name {
        Some(ref raw) => {
            let name = raw.trim();
            if name.is_empty() {
                return Err(AppError::coded("merchant.name-required", "商户名不能为空"));
            }
            name.to_string()
        }
        None => existing.name,
    };
    if merchant_name_taken(conn, &name, Some(id))? {
        return Err(AppError::codedp(
            "merchant.already-exists",
            format!("商户已存在: {name}"),
            &[&name],
        ));
    }

    conn.execute(
        "UPDATE merchants SET name=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
        rusqlite::params![&name, now_iso(), device_id(conn)?, id],
    )?;
    Ok(name)
}

pub fn delete_merchant(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_local(conn, MerchantCommand::Delete { id: id.to_string() })
    })
}

fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM merchants WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::codedp_not_found(
            "merchant.not-found",
            format!("商户不存在: {id}"),
            &[id],
        ));
    }
    conn.execute(
        "UPDATE merchants SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

pub fn transaction_counts(conn: &Connection) -> Result<Vec<MerchantTransactionCount>> {
    query_all(
        conn,
        "SELECT m.id, COUNT(t.id) AS transaction_count \
         FROM merchants m \
         LEFT JOIN transactions t ON t.merchant_id=m.id AND t.is_deleted=0 \
         GROUP BY m.id ORDER BY m.name, m.created_at",
        [],
    )
}

pub fn find_merchant_by_name(conn: &Connection, name: &str) -> Result<Option<String>> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM merchants WHERE name=?1 AND is_deleted=0",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id)
}

pub fn create_merchant_by_name(conn: &Connection, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::coded("merchant.name-required", "商户名不能为空"));
    }
    if let Some(id) = find_merchant_by_name(conn, name)? {
        return Ok(id);
    }
    create_merchant(
        conn,
        MerchantInput {
            name: name.to_string(),
        },
    )
}

fn merchant_name_taken(conn: &Connection, name: &str, exclude_id: Option<&str>) -> Result<bool> {
    let found: bool = conn
        .query_row(
            "SELECT 1 FROM merchants WHERE name=?1 AND is_deleted=0 AND (?2 IS NULL OR id<>?2)",
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
        &MerchantUpdateInput {
            name: Some(name.to_string()),
        },
    )?;
    Ok(())
}

pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    write_delete(conn, id)
}

pub fn install_merchant_hooks() {
    ledger_transaction::seams::merchant::register_merchant_hooks(
        ledger_transaction::seams::merchant::MerchantNameHooks {
            find_by_name: find_merchant_by_name,
            create_by_name: create_merchant_by_name,
        },
    );
}
