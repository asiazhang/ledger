#![allow(clippy::unreachable)]

use chrono::Local;
use tauri::State;

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_budget as budget_domain;
use ledger_budget::{Budget, BudgetAnnualSummary, BudgetInput, BudgetProgress, BudgetUpdateInput};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;

#[tauri::command]
pub async fn list_budgets(db: State<'_, DbState>) -> Result<Vec<Budget>> {
    let conn = db.read_handle();
    read_entry("list_budgets", conn, move |conn| {
        budget_domain::list_budgets(conn)
    })
    .await
}

#[tauri::command]
pub async fn create_budget(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: BudgetInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_budget",
        conn,
        Some(&app),
        WriteOp::CreateBudget,
        move |conn| budget_domain::crud::create_budget(conn, &input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_budget(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: BudgetUpdateInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_budget",
        conn,
        Some(&app),
        WriteOp::UpdateBudget,
        move |conn| {
            budget_domain::crud::update_budget(conn, &id, input.amount_cents, input.period)
                .map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn delete_budget(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "delete_budget",
        conn,
        Some(&app),
        WriteOp::DeleteBudget,
        move |conn| budget_domain::crud::delete_budget(conn, &id).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn budget_progress(db: State<'_, DbState>) -> Result<Vec<BudgetProgress>> {
    let conn = db.read_handle();
    read_entry("budget_progress", conn, move |conn| {
        budget_domain::budget_progress_rows(conn, Local::now().date_naive())
    })
    .await
}

#[tauri::command]
pub async fn budget_annual_summary(db: State<'_, DbState>) -> Result<BudgetAnnualSummary> {
    let conn = db.read_handle();
    read_entry("budget_annual_summary", conn, move |conn| {
        budget_domain::budget_annual_summary(conn, Local::now().date_naive())
    })
    .await
}
