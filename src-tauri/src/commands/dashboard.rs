#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry_on_write;
use ledger_dashboard as dashboard_domain;
use ledger_dashboard::DashboardOverview;
use ledger_infra::db::DbState;
use ledger_infra::error::Result;

#[tauri::command]
pub async fn dashboard_overview(db: State<'_, DbState>) -> Result<DashboardOverview> {
    let conn = db.write_handle();
    read_entry_on_write("dashboard_overview", conn, move |conn| {
        dashboard_domain::query_dashboard_overview(conn)
    })
    .await
}
