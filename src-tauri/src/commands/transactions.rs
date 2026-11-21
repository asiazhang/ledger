#![allow(clippy::unreachable)]

use tauri::{AppHandle, Runtime, State};

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;
use ledger_transaction as transaction_domain;
use ledger_transaction::{
    CreateTransactionResult, TransactionInput, TransactionListFilter, TransactionListResult,
    TransactionOrderSummary, UpdateTransactionInput,
};

#[tauri::command]
pub async fn list_transactions(
    db: State<'_, DbState>,
    filter: Option<TransactionListFilter>,
) -> Result<TransactionListResult> {
    let conn = db.read_handle();
    read_entry("list_transactions", conn, move |conn| {
        let filter = filter.unwrap_or_default();
        transaction_domain::list_transactions_internal(conn, &filter)
    })
    .await
}

#[tauri::command]
pub async fn create_transaction<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    input: TransactionInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_transaction",
        conn,
        Some(&app),
        WriteOp::CreateTransaction,
        move |conn| {
            transaction_domain::create(conn, input)
                .map(|write| Outcome::Evidenced(write.id, write.evidence))
        },
    )
    .await
}

#[tauri::command]
pub async fn create_transactions(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    inputs: Vec<TransactionInput>,
) -> Result<Vec<CreateTransactionResult>> {
    let conn = db.write_handle();
    write_entry(
        "create_transactions",
        conn,
        Some(&app),
        WriteOp::BatchCreateTransactions,
        move |conn| {
            transaction_domain::TransactionBatch::run(conn, inputs, false)
                .map(|outcome| Outcome::Evidenced(outcome.results, outcome.evidence))
        },
    )
    .await
}

#[tauri::command]
pub async fn update_transaction(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: UpdateTransactionInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_transaction",
        conn,
        Some(&app),
        WriteOp::UpdateTransaction,
        move |conn| {
            transaction_domain::update(conn, &id, input.into())
                .map(|evidence| Outcome::Evidenced((), evidence))
        },
    )
    .await
}

#[tauri::command]
pub async fn delete_transaction(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "delete_transaction",
        conn,
        Some(&app),
        WriteOp::DeleteTransaction,
        move |conn| transaction_domain::delete(conn, &id).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn get_transaction_order_summary(
    db: State<'_, DbState>,
    source_order_no: String,
) -> Result<TransactionOrderSummary> {
    let conn = db.read_handle();
    read_entry("get_transaction_order_summary", conn, move |conn| {
        transaction_domain::get_transaction_order_summary(conn, &source_order_no)
    })
    .await
}
