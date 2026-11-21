#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use ledger_currencies as currency_domain;
use ledger_currencies::Currency;
use ledger_infra::db::DbState;
use ledger_infra::error::Result;

#[tauri::command]
pub async fn list_currencies(db: State<'_, DbState>) -> Result<Vec<Currency>> {
    let conn = db.read_handle();
    read_entry("list_currencies", conn, move |conn| {
        currency_domain::list_currencies(conn)
    })
    .await
}
