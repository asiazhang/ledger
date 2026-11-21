#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;
use ledger_merchants as merchant_domain;
use ledger_merchants::{Merchant, MerchantInput, MerchantTransactionCount, MerchantUpdateInput};

#[tauri::command]
pub async fn list_merchants(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Merchant>> {
    let conn = db.read_handle();
    read_entry("list_merchants", conn, move |conn| {
        merchant_domain::list_merchants(conn, include_deleted.unwrap_or(false))
    })
    .await
}

#[tauri::command]
pub async fn list_merchant_transaction_counts(
    db: State<'_, DbState>,
) -> Result<Vec<MerchantTransactionCount>> {
    let conn = db.read_handle();
    read_entry("list_merchant_transaction_counts", conn, move |conn| {
        merchant_domain::transaction_counts(conn)
    })
    .await
}

#[tauri::command]
pub async fn create_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: MerchantInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_merchant",
        conn,
        Some(&app),
        WriteOp::CreateMerchant,
        move |conn| merchant_domain::create_merchant(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: MerchantUpdateInput,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "update_merchant",
        conn,
        Some(&app),
        WriteOp::UpdateMerchant,
        move |conn| merchant_domain::update_merchant(conn, &id, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "delete_merchant",
        conn,
        Some(&app),
        WriteOp::DeleteMerchant,
        move |conn| merchant_domain::delete_merchant(conn, &id).map(Outcome::Silent),
    )
    .await
}
