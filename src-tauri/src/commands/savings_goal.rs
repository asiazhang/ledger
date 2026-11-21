#![allow(clippy::unreachable)]

use tauri::{AppHandle, Runtime, State};

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;
use ledger_savings_goal::{
    self as savings_goal_domain, SavingsGoalInput, SavingsGoalProgress, SavingsGoalUpdateInput,
};

#[tauri::command]
pub async fn savings_goal_progress(db: State<'_, DbState>) -> Result<Vec<SavingsGoalProgress>> {
    let conn = db.read_handle();
    read_entry("savings_goal_progress", conn, move |conn| {
        savings_goal_domain::list_savings_goal_progress(conn, chrono::Local::now().date_naive())
    })
    .await
}

#[tauri::command]
pub async fn create_savings_goal<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    input: SavingsGoalInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_savings_goal",
        conn,
        Some(&app),
        WriteOp::CreateSavingsGoal,
        move |conn| savings_goal_domain::create_savings_goal(conn, &input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_savings_goal<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    id: String,
    input: SavingsGoalUpdateInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_savings_goal",
        conn,
        Some(&app),
        WriteOp::UpdateSavingsGoal,
        move |conn| {
            savings_goal_domain::update_savings_goal(conn, &id, &input).map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn archive_savings_goal<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "archive_savings_goal",
        conn,
        Some(&app),
        WriteOp::ArchiveSavingsGoal,
        move |conn| savings_goal_domain::archive_savings_goal(conn, &id).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn unarchive_savings_goal<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "unarchive_savings_goal",
        conn,
        Some(&app),
        WriteOp::UnarchiveSavingsGoal,
        move |conn| savings_goal_domain::unarchive_savings_goal(conn, &id).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_savings_goal<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "delete_savings_goal",
        conn,
        Some(&app),
        WriteOp::DeleteSavingsGoal,
        move |conn| savings_goal_domain::delete_savings_goal(conn, &id).map(Outcome::Silent),
    )
    .await
}
