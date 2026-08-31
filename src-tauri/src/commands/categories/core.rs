//! 分类域核心逻辑（issue #91）：CRUD / 幂等创建 / 软删除。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域零感知，
//! 写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。

use rusqlite::{Connection, OptionalExtension};

use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Category, CategoryInput};

pub fn list_categories_internal(conn: &Connection) -> Result<Vec<Category>> {
    query_all(
        conn,
        "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
         FROM categories WHERE is_deleted=0 ORDER BY kind, sort_order, created_at",
        [],
    )
}

pub fn create_category_internal(conn: &Connection, input: CategoryInput) -> Result<String> {
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0)",
        rusqlite::params![
            id,
            input.name,
            input.kind,
            input.parent_id,
            input.icon,
            0,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

/// 按自然键（name + kind + parent_id）幂等创建分类：已存在（未删除）时返回已有 id，
/// 不重复插入、不报错。供 HTTP 导入 API 使用。
pub fn create_category_idempotent_internal(
    conn: &Connection,
    input: CategoryInput,
) -> Result<String> {
    if let Some(id) = find_category_by_natural_key(conn, &input)? {
        return Ok(id);
    }
    create_category_internal(conn, input)
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

/// 软删除分类（`is_deleted=1`）。不校验引用（与 UI 行为一致）。不存在的 id
/// 返回 `AppError::NotFound`（HTTP 侧映射 404）。IPC 与 HTTP 端点共用本函数。
pub fn delete_category_internal(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM categories WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::coded_not_found(
            "category.not-found",
            format!("分类不存在: {id}"),
        ));
    }
    conn.execute(
        "UPDATE categories SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}
