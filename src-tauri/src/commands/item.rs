#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;
use ledger_item::domain;
use ledger_item::{ItemDailyCost, ItemDisposeInput, ItemInput, ItemWithDailyCost};

#[tauri::command]
pub async fn list_items(db: State<'_, DbState>) -> Result<Vec<ItemWithDailyCost>> {
    let conn = db.read_handle();
    read_entry("list_items", conn, domain::list_items).await
}

#[tauri::command]
pub async fn calculate_item_cost(
    db: State<'_, DbState>,
    id: String,
    reference_date: Option<String>,
) -> Result<ItemDailyCost> {
    let conn = db.read_handle();
    read_entry("calculate_item_cost", conn, move |conn| {
        domain::calculate_item_cost(conn, &id, reference_date.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn create_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ItemInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_item",
        conn,
        Some(&app),
        WriteOp::CreateItem,
        move |conn| domain::create_item(conn, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_item",
        conn,
        Some(&app),
        WriteOp::UpdateItem,
        move |conn| domain::update_item(conn, &id, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn dispose_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemDisposeInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "dispose_item",
        conn,
        Some(&app),
        WriteOp::DisposeItem,
        move |conn| domain::dispose_item(conn, &id, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_item(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "delete_item",
        conn,
        Some(&app),
        WriteOp::DeleteItem,
        move |conn| domain::delete_item(conn, &id, &mut || {}).map(Outcome::Silent),
    )
    .await
}
