#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_transaction as transaction_domain;
use ledger_transaction::TransactionSearchResult;

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn search_transactions(
    db: State<'_, DbState>,
    query: String,
    page: Option<usize>,
    page_size: Option<usize>,
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Result<TransactionSearchResult> {
    let conn = db.read_handle();
    read_entry("search_transactions", conn, move |conn| {
        transaction_domain::search_transactions_internal(
            conn,
            &query,
            page.unwrap_or(1),
            page_size.unwrap_or(20),
            amount_min_cents,
            amount_max_cents,
            date_from.as_deref(),
            date_to.as_deref(),
        )
    })
    .await
}
