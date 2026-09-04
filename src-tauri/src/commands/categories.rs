//! IPC 命令壳 · 分类（Category）。
//!
//! 只负责参数解包、事务边界与失效信号发射；分类域行为位于 [`crate::categories`]。
//! 注册路径与前端调用保持不变。
//!
//! 全部命令 async 化（形状乙，spec #498 / #501）：DB 调用经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程；
//! 写路径仍在连接层统一写入口内置脏（ADR-0032 语义零改动）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::categories as category_domain;
use crate::categories::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};
use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 分类列表：默认仅未删除；`include_deleted=true` 返回含软删全量（issue #377，先例商户）。
#[tauri::command]
pub async fn list_categories(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Category>> {
    let conn = db.conn.clone();
    run_db("list_categories", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        category_domain::list_categories(&conn, include_deleted.unwrap_or(false))
    })
    .await
}

#[tauri::command]
pub async fn create_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: CategoryInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    let id = run_db("create_category", move || {
        crate::db::write(&conn, |conn| category_domain::create_category(conn, input))
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::CreateCategory, WriteEvidence::None);
    Ok(id)
}

#[tauri::command]
pub async fn update_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: CategoryUpdateInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("update_category", move || {
        crate::db::write(&conn, |conn| {
            category_domain::update_category(conn, &id, input)
        })
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::UpdateCategory, WriteEvidence::None);
    Ok(())
}

#[tauri::command]
pub async fn reorder_categories(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    items: Vec<ReorderItem>,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("reorder_categories", move || {
        crate::db::write(&conn, |conn| {
            category_domain::reorder_categories(conn, items)
        })
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::ReorderCategories, WriteEvidence::None);
    Ok(())
}

#[tauri::command]
pub async fn delete_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("delete_category", move || {
        crate::db::write(&conn, |conn| category_domain::delete_category(conn, &id))
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::DeleteCategory, WriteEvidence::None);
    Ok(())
}
