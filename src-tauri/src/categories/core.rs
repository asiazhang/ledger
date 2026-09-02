//! 分类域核心逻辑（issue #91 域内收口，#404 自命令壳层迁入）：CRUD / 幂等创建 /
//! 软删除 / 两级分类校验 / 预算删除守卫 / 排序重排。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域零感知，
//! 写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。

use rusqlite::{Connection, OptionalExtension};

use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};

/// 分类列表：默认仅未删除；`include_deleted=true` 返回含软删全量（issue #377）。
/// 软删分类不可再被选择，但历史交易引用照常存在——含软删列表供前端下钻校验映射
/// 与历史显示拆分在用/软删（先例商户 issue #191）。
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

/// 软删除分类（`is_deleted=1`）。删除守卫（issue #355）：分类自身名下存在未删除预算时拒绝，
/// 中文文案引导先删对应预算；只查分类自身（删父分类不检查其子分类名下预算，
/// 与「删除只影响该分类自身」一致）；仅剩软删除预算不阻拦。不存在的 id
/// 返回 `AppError::NotFound`（HTTP 侧映射 404）。IPC 与 HTTP 端点共用本函数，
/// 守卫一处生效。
pub fn delete_category(conn: &Connection, id: &str) -> Result<()> {
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
    // 预算删除守卫（issue #355）：防止误删分类留下孤儿预算。
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
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

/// 编辑分类（`name` / `icon` / `parent_id` 可选字段，未传保持原值）：两级分类校验——
/// 自身不可为父、父分类须存在（未删除）且 `kind` 与自身一致——通过后落库。
/// 分类名不在搜索范围内（ADR-0027），且搜索无索引，改名无需任何后续处理。
pub fn update_category(conn: &Connection, id: &str, input: CategoryUpdateInput) -> Result<()> {
    let existing: Category = query_all(
        conn,
        "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
         FROM categories WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| AppError::codedp_not_found("category.not-found", format!("分类不存在: {id}"), &[id]))?;

    let parent_id = input.parent_id.unwrap_or(existing.parent_id);

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

    let name = input.name.unwrap_or(existing.name);
    let icon = input.icon.or(existing.icon);

    conn.execute(
        "UPDATE categories SET name=?1, icon=?2, parent_id=?3, updated_at=?4, version=version+1, device_id=?5 WHERE id=?6",
        rusqlite::params![name, icon, parent_id, now_iso(), device_id(), id],
    )?;
    Ok(())
}

/// 排序重排：按提交顺序逐行落 `sort_order`（`updated_at`/`version`/`device_id`
/// 同步递增）；IPC 与 HTTP 侧共用本函数，排序语义一处生效。
pub fn reorder_categories(conn: &Connection, items: Vec<ReorderItem>) -> Result<()> {
    let now = now_iso();
    let did = device_id();
    for item in &items {
        conn.execute(
            "UPDATE categories SET sort_order=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![item.sort_order, now, did, item.id],
        )?;
    }
    Ok(())
}
