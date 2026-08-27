//! 分类（issue #91）：命令外壳 + 核心逻辑 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `core`：核心逻辑——分类 CRUD/幂等创建/软删除（原 categories.rs 非命令部分，保持原状不拆分）；
//! - `tests`：原内嵌测试外迁。
//!
//! `update_category` / `reorder_categories` 逻辑内嵌于命令本身（主代码未超阈值，
//! 不拆核心逻辑），随命令外壳一并落在本模块入口。
//! 对外暴露的命令与 `*_internal` 复用函数经 `commands/mod.rs` 的 `pub use categories::*`
//! 重导出，注册路径与前端/api_server 调用零改动。

mod core;
#[cfg(test)]
mod tests;

use tauri::State;

use crate::db::query::query_all;
use crate::db::{DbState, device_id, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};

pub use core::*;

#[tauri::command]
pub fn list_categories(db: State<'_, DbState>) -> Result<Vec<Category>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    core::list_categories_internal(&conn)
}

#[tauri::command]
pub fn create_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: CategoryInput,
) -> Result<String> {
    let id = {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        core::create_category_internal(&conn, input)?
    };
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_reference_changed(&app, "create_category");
    Ok(id)
}

#[tauri::command]
pub fn update_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: CategoryUpdateInput,
) -> Result<()> {
    {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let now = now_iso();
        let did = device_id();

        let existing: Category = query_all(
        &conn,
        "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
         FROM categories WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| AppError::NotFound(format!("分类不存在: {id}")))?;

        let parent_id = input.parent_id.unwrap_or(existing.parent_id);

        if let Some(ref pid) = parent_id {
            if *pid == id {
                return Err(AppError::Invalid("自身不能作为父分类".into()));
            }
            let parent: Category = query_all(
            &conn,
            "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
             FROM categories WHERE id=?1 AND is_deleted=0",
            rusqlite::params![pid],
        )?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound(format!("父分类不存在: {pid}")))?;
            if parent.kind != existing.kind {
                return Err(AppError::Invalid("父分类类型需一致".into()));
            }
        }

        let name = input.name.unwrap_or(existing.name);
        let icon = input.icon.or(existing.icon);

        conn.execute(
        "UPDATE categories SET name=?1, icon=?2, parent_id=?3, updated_at=?4, version=version+1, device_id=?5 WHERE id=?6",
        rusqlite::params![name, icon, parent_id, now, did, id],
    )?;
        // 脏标记挂钩（issue #126）：分类更新成功即置脏。
        crate::auto_backup::on_write(&conn);
        // 分类改名不影响搜索索引（分类名不在搜索内容中，V005 收窄后无需重建）
    }
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_reference_changed(&app, "update_category");
    Ok(())
}

#[tauri::command]
pub fn reorder_categories(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    items: Vec<ReorderItem>,
) -> Result<()> {
    {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let now = now_iso();
        let did = device_id();
        for item in &items {
            conn.execute(
                "UPDATE categories SET sort_order=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
                rusqlite::params![item.sort_order, now, did, item.id],
            )?;
        }
        // 脏标记挂钩（issue #126）：排序调整成功即置脏。
        crate::auto_backup::on_write(&conn);
    }
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_reference_changed(&app, "reorder_categories");
    Ok(())
}

#[tauri::command]
pub fn delete_category(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        core::delete_category_internal(&conn, &id)?;
    }
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_reference_changed(&app, "delete_category");
    Ok(())
}
