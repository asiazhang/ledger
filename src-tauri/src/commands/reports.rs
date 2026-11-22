#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_reports as reports_domain;
use ledger_reports::{CategoryShare, DateRange, MerchantSharesReport, MonthlySummary};

#[tauri::command]
pub async fn monthly_summary(
    db: State<'_, DbState>,
    year: i64,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<MonthlySummary>> {
    let conn = db.read_handle();
    read_entry("monthly_summary", conn, move |conn| {
        reports_domain::monthly_summary_rows(conn, year, from.as_deref(), to.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn merchant_shares(
    db: State<'_, DbState>,
    year: i64,
    from: Option<String>,
    to: Option<String>,
    top_n: Option<i64>,
) -> Result<MerchantSharesReport> {
    let conn = db.read_handle();
    read_entry("merchant_shares", conn, move |conn| {
        reports_domain::merchant_shares_report(conn, year, from.as_deref(), to.as_deref(), top_n)
    })
    .await
}

#[tauri::command]
pub async fn report_date_range(db: State<'_, DbState>) -> Result<DateRange> {
    let conn = db.read_handle();
    read_entry("report_date_range", conn, move |conn| {
        reports_domain::query_report_date_range(conn)
    })
    .await
}

#[tauri::command]
pub async fn category_shares(
    db: State<'_, DbState>,
    kind: String,
    month: Option<String>,
    year: Option<i64>,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<CategoryShare>> {
    let conn = db.read_handle();
    read_entry("category_shares", conn, move |conn| {
        reports_domain::category_shares_rows(
            conn,
            &kind,
            month.as_deref(),
            year,
            from.as_deref(),
            to.as_deref(),
        )
    })
    .await
}
