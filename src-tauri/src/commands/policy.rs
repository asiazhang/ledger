#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;
use ledger_policy::{self as policy_domain, Policy, PolicyInput, PolicyStats};

#[tauri::command]
pub async fn list_policies(db: State<'_, DbState>) -> Result<Vec<Policy>> {
    let conn = db.read_handle();
    read_entry("list_policies", conn, move |conn| {
        policy_domain::list_policies(conn)
    })
    .await
}

#[tauri::command]
pub async fn list_policy_stats(db: State<'_, DbState>) -> Result<Vec<PolicyStats>> {
    let conn = db.read_handle();
    read_entry("list_policy_stats", conn, move |conn| {
        policy_domain::policy_stats(conn, chrono::Local::now().date_naive())
    })
    .await
}

#[tauri::command]
pub async fn create_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: PolicyInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_policy",
        conn,
        Some(&app),
        WriteOp::CreatePolicy,
        move |conn| policy_domain::create_policy(conn, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PolicyInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_policy",
        conn,
        Some(&app),
        WriteOp::UpdatePolicy,
        move |conn| policy_domain::update_policy(conn, &id, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "delete_policy",
        conn,
        Some(&app),
        WriteOp::DeletePolicy,
        move |conn| policy_domain::delete_policy(conn, &id, &mut || {}).map(Outcome::Silent),
    )
    .await
}
