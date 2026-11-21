#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_investment as investment_domain;
use ledger_investment::FinancialFreedomOverview;

#[tauri::command]
pub async fn financial_freedom(db: State<'_, DbState>) -> Result<FinancialFreedomOverview> {
    let conn = db.read_handle();
    read_entry("financial_freedom", conn, move |conn| {
        investment_domain::query_financial_freedom(conn)
    })
    .await
}
