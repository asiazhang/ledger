//! IPC 命令壳 · 分类（Category）。
//!
//! 只负责参数解包、事务边界与失效信号发射；分类域行为位于 [`crate::categories`]。
//! 注册路径与前端调用保持不变。

use tauri::State;

use crate::categories as category_domain;
use crate::categories::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};
use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 分类列表：默认仅未删除；`include_deleted=true` 返回含软删全量（issue #377，先例商户）。
#[tauri::command]
pub fn list_categories(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Category>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    category_domain::list_categories(&conn, include_deleted.unwrap_or(false))
}

#[tauri::command]
pub fn create_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: CategoryInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = db.write(|conn| category_domain::create_category(conn, input))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::CreateCategory, WriteEvidence::None);
    Ok(id)
}

#[tauri::command]
pub fn update_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: CategoryUpdateInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| category_domain::update_category(conn, &id, input))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::UpdateCategory, WriteEvidence::None);
    Ok(())
}

#[tauri::command]
pub fn reorder_categories(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    items: Vec<ReorderItem>,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| category_domain::reorder_categories(conn, items))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::ReorderCategories, WriteEvidence::None);
    Ok(())
}

#[tauri::command]
pub fn delete_category(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| category_domain::delete_category(conn, &id))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::DeleteCategory, WriteEvidence::None);
    Ok(())
}
