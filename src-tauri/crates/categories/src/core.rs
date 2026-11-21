use rusqlite::{Connection, OptionalExtension};

use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;

use super::command::{CategoryCommand, CategoryCommandRow, record_local};
use super::model::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};

pub fn list_categories(conn: &Connection, include_deleted: bool) -> Result<Vec<Category>> {
    let where_clause = if include_deleted {
        ""
    } else {
        "WHERE is_deleted=0"
    };
    query_all(
        conn,
        &format!(
            "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
             FROM categories {where_clause} ORDER BY kind, sort_order, created_at"
        ),
        [],
    )
}

pub fn create_category(conn: &Connection, input: CategoryInput) -> Result<String> {
    ensure_transaction(conn, || {
        let row = CategoryCommandRow {
            name: input.name.clone(),
            kind: input.kind.clone(),
            parent_id: input.parent_id.clone(),
            icon: input.icon.clone(),
        };
        let id = write_create(conn, &new_uuid(), &row)?;
        record_local(
            conn,
            CategoryCommand::Create {
                id: id.clone(),
                row,
            },
        )?;
        Ok(id)
    })
}

fn write_create(conn: &Connection, id: &str, row: &CategoryCommandRow) -> Result<String> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,0,?6,?7,1,?8,0)",
        rusqlite::params![
            id,
            row.name,
            row.kind,
            row.parent_id,
            row.icon,
            now,
            now,
            device_id(conn)?
        ],
    )?;
    Ok(id.to_string())
}

pub fn create_category_idempotent(conn: &Connection, input: CategoryInput) -> Result<String> {
    if let Some(id) = find_category_by_natural_key(conn, &input)? {
        return Ok(id);
    }
    create_category(conn, input)
}

fn find_category_by_natural_key(
    conn: &Connection,
    input: &CategoryInput,
) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM categories \
         WHERE name=?1 AND kind=?2 AND parent_id IS ?3 AND is_deleted=0 LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![input.name, input.kind, input.parent_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

pub fn delete_category(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_local(conn, CategoryCommand::Delete { id: id.to_string() })
    })
}

fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM categories WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::codedp_not_found(
            "category.not-found",
            format!("分类不存在: {id}"),
            &[id],
        ));
    }
    let has_budget: bool = conn
        .query_row(
            "SELECT 1 FROM budgets WHERE category_id=?1 AND is_deleted=0 LIMIT 1",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if has_budget {
        return Err(AppError::coded(
            "category.has-budget",
            "该分类名下存在预算，请先删除对应预算后再删除分类",
        ));
    }
    conn.execute(
        "UPDATE categories SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

pub fn update_category(conn: &Connection, id: &str, input: CategoryUpdateInput) -> Result<()> {
    ensure_transaction(conn, || {
        let (name, icon, parent_id) = write_update(conn, id, &input)?;
        record_local(
            conn,
            CategoryCommand::Update {
                id: id.to_string(),
                name,
                icon,
                parent_id,
            },
        )
    })
}

fn write_update(
    conn: &Connection,
    id: &str,
    input: &CategoryUpdateInput,
) -> Result<(String, Option<String>, Option<String>)> {
    let existing: Category = query_all(
        conn,
        "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
         FROM categories WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| AppError::codedp_not_found("category.not-found", format!("分类不存在: {id}"), &[id]))?;

    let parent_id = input.parent_id.clone().unwrap_or(existing.parent_id);

    if let Some(ref pid) = parent_id {
        if *pid == id {
            return Err(AppError::coded(
                "category.self-parent",
                "自身不能作为父分类",
            ));
        }
        let parent: Category = query_all(
            conn,
            "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
             FROM categories WHERE id=?1 AND is_deleted=0",
            rusqlite::params![pid],
        )?
        .into_iter()
        .next()
        .ok_or_else(|| {
            AppError::codedp_not_found("category.parent-not-found", format!("父分类不存在: {pid}"), &[pid])
        })?;
        if parent.kind != existing.kind {
            return Err(AppError::coded(
                "category.parent-kind-mismatch",
                "父分类类型需一致",
            ));
        }
    }

    let name = input.name.clone().unwrap_or(existing.name);
    let icon = input.icon.clone().unwrap_or(existing.icon);

    conn.execute(
        "UPDATE categories SET name=?1, icon=?2, parent_id=?3, updated_at=?4, version=version+1, device_id=?5 WHERE id=?6",
        rusqlite::params![&name, &icon, &parent_id, now_iso(), device_id(conn)?, id],
    )?;
    Ok((name, icon, parent_id))
}

pub fn reorder_categories(conn: &Connection, items: Vec<ReorderItem>) -> Result<()> {
    ensure_transaction(conn, || {
        write_reorder(conn, &items)?;
        record_local(conn, CategoryCommand::Reorder { items })
    })
}

pub(crate) fn write_reorder(conn: &Connection, items: &[ReorderItem]) -> Result<()> {
    let now = now_iso();
    let did = device_id(conn)?;
    for item in items {
        conn.execute(
            "UPDATE categories SET sort_order=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![item.sort_order, now, did, item.id],
        )?;
    }
    Ok(())
}

pub(crate) fn replay_create(conn: &Connection, id: &str, row: &CategoryCommandRow) -> Result<()> {
    write_create(conn, id, row)?;
    Ok(())
}

pub(crate) fn replay_update(
    conn: &Connection,
    id: &str,
    name: &str,
    icon: Option<&str>,
    parent_id: Option<&str>,
) -> Result<()> {
    write_update(
        conn,
        id,
        &CategoryUpdateInput {
            name: Some(name.to_string()),
            icon: Some(icon.map(String::from)),
            parent_id: Some(parent_id.map(String::from)),
        },
    )?;
    Ok(())
}

pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    write_delete(conn, id)
}
