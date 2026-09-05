//! IPC 命令壳 · 分类（Category）。
//!
//! 只负责参数解包与统一写入口一行调用；分类域行为位于 [`crate::categories`]。
//! 注册路径与前端调用保持不变。
//!
//! 全部命令 async 化（形状乙，spec #498 / #501）；写命令经壳层统一写入口
//! [`crate::write_entry::write_entry`]（ADR-0073）：仪式（锁、事务、置脏、信号）
//! 内化单点，参考写入成功发参考失效信号（映射单点判定，ADR-0044）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::categories as category_domain;
use crate::categories::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};
use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

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
    let conn = db.conn.clone();
    write_entry(
        "create_category",
        conn,
        Some(&app),
        WriteOp::CreateCategory,
        move |conn| category_domain::create_category(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: CategoryUpdateInput,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "update_category",
        conn,
        Some(&app),
        WriteOp::UpdateCategory,
        move |conn| category_domain::update_category(conn, &id, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn reorder_categories(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    items: Vec<ReorderItem>,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "reorder_categories",
        conn,
        Some(&app),
        WriteOp::ReorderCategories,
        move |conn| category_domain::reorder_categories(conn, items).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "delete_category",
        conn,
        Some(&app),
        WriteOp::DeleteCategory,
        move |conn| category_domain::delete_category(conn, &id).map(Outcome::Silent),
    )
    .await
}
