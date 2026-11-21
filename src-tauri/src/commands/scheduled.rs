#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;
use ledger_scheduled as scheduled_domain;
use ledger_scheduled::{
    CreateScheduledInput, ExecuteOccurrenceInput, ScheduledTransactionDetail,
    ScheduledTransactionWithExt, SubscriptionSpendOverview, UpdateStatusInput,
    UpdateSubscriptionInput,
};

#[tauri::command]
pub async fn create_scheduled_transaction(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: CreateScheduledInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_scheduled_transaction",
        conn,
        Some(&app),
        WriteOp::CreateScheduledTransaction,
        move |conn| scheduled_domain::create_plan(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn list_scheduled_transactions(
    db: State<'_, DbState>,
) -> Result<Vec<ScheduledTransactionWithExt>> {
    let conn = db.read_handle();
    read_entry("list_scheduled_transactions", conn, move |conn| {
        scheduled_domain::list_plans(conn)
    })
    .await
}

#[tauri::command]
pub async fn get_scheduled_transaction_detail(
    db: State<'_, DbState>,
    id: String,
) -> Result<ScheduledTransactionDetail> {
    let conn = db.read_handle();
    read_entry("get_scheduled_transaction_detail", conn, move |conn| {
        scheduled_domain::get_plan_detail(conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn update_scheduled_transaction_status(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: UpdateStatusInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_scheduled_transaction_status",
        conn,
        Some(&app),
        WriteOp::UpdateScheduledTransactionStatus,
        move |conn| {
            scheduled_domain::update_plan_status(conn, &input.id, input.new_status)
                .map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn update_scheduled_subscription(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: UpdateSubscriptionInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_scheduled_subscription",
        conn,
        Some(&app),
        WriteOp::UpdateScheduledSubscription,
        move |conn| scheduled_domain::update_subscription(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn execute_scheduled_occurrence(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ExecuteOccurrenceInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "execute_scheduled_occurrence",
        conn,
        Some(&app),
        WriteOp::ExecuteScheduledOccurrence,
        move |conn| {
            scheduled_domain::execute_occurrence(conn, &input.occurrence_id).map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn expand_scheduled_occurrences(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<Vec<String>> {
    let conn = db.write_handle();
    write_entry(
        "expand_scheduled_occurrences",
        conn,
        Some(&app),
        WriteOp::ExpandScheduledOccurrences,
        move |conn| scheduled_domain::expand_occurrences(conn, &id).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn subscription_spend_overview(
    db: State<'_, DbState>,
) -> Result<SubscriptionSpendOverview> {
    let conn = db.read_handle();
    read_entry("subscription_spend_overview", conn, move |conn| {
        scheduled_domain::query_subscription_spend(conn, chrono::Local::now().date_naive())
    })
    .await
}

#[tauri::command]
pub fn set_auto_execution_enabled(enabled: bool) -> Result<()> {
    ledger_scheduled::auto_run::set_enabled(enabled);
    Ok(())
}
