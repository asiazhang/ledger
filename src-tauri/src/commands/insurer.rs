#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;
use ledger_policy as policy_domain;
use ledger_policy::{Insurer, InsurerInput, InsurerUpdateInput};

#[tauri::command]
pub async fn list_insurers(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Insurer>> {
    let conn = db.read_handle();
    read_entry("list_insurers", conn, move |conn| {
        policy_domain::list_insurers(conn, include_deleted.unwrap_or(false))
    })
    .await
}

#[tauri::command]
pub async fn create_insurer(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: InsurerInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_insurer",
        conn,
        Some(&app),
        WriteOp::CreateInsurer,
        move |conn| policy_domain::create_insurer(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_insurer(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: InsurerUpdateInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_insurer",
        conn,
        Some(&app),
        WriteOp::UpdateInsurer,
        move |conn| policy_domain::update_insurer(conn, &id, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_insurer(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "delete_insurer",
        conn,
        Some(&app),
        WriteOp::DeleteInsurer,
        move |conn| policy_domain::delete_insurer(conn, &id).map(Outcome::Silent),
    )
    .await
}
