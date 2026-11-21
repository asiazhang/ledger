#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_categories as category_domain;
use ledger_categories::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;

#[tauri::command]
pub async fn list_categories(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Category>> {
    let conn = db.read_handle();
    read_entry("list_categories", conn, move |conn| {
        category_domain::list_categories(conn, include_deleted.unwrap_or(false))
    })
    .await
}

#[tauri::command]
pub async fn create_category(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: CategoryInput,
) -> Result<String> {
    let conn = db.write_handle();
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
    let conn = db.write_handle();
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
    let conn = db.write_handle();
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
    let conn = db.write_handle();
    write_entry(
        "delete_category",
        conn,
        Some(&app),
        WriteOp::DeleteCategory,
        move |conn| category_domain::delete_category(conn, &id).map(Outcome::Silent),
    )
    .await
}
