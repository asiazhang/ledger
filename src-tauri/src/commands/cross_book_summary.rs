#![allow(clippy::unreachable)]

use tauri::{AppHandle, Runtime, State};

use crate::commands::data_location::default_data_dir;
use crate::cross_book_summary::{
    CrossBookInvestmentSummary, collect_other_books, read_active_book_and_merge,
};
use crate::shell_support::read_entry::read_entry;
use ledger_infra::db::{DbState, book_registry, run_db};
use ledger_infra::error::{AppError, Result};

const REGISTRY_UNAVAILABLE_MESSAGE: &str = "账本注册表尚未就绪，请重启应用后再试";

#[tauri::command]
pub async fn cross_book_investment_summary<R: Runtime>(
    app: AppHandle<R>,
    db: State<'_, DbState>,
) -> Result<CrossBookInvestmentSummary> {
    let default_dir = default_data_dir(&app)?;
    let registry = run_db(
        "cross_book_summary:registry",
        move || match book_registry::read_registry(&default_dir) {
            book_registry::RegistryRead::Resolved(registry) => Ok(registry),
            _ => Err(AppError::coded(
                "book.registry-unavailable",
                REGISTRY_UNAVAILABLE_MESSAGE,
            )),
        },
    )
    .await?;
    let books = registry.books;
    let active_id = registry.active_id;

    let active_schema_version = read_entry(
        "cross_book_summary:schema_version",
        db.read_handle(),
        ledger_infra::db::schema_version,
    )
    .await?;

    let (rows, readings) = run_db("cross_book_summary:books", move || {
        let (rows, readings) = collect_other_books(&books, &active_id, active_schema_version);
        Ok((rows, readings))
    })
    .await?;

    read_entry("cross_book_summary", db.read_handle(), move |conn| {
        read_active_book_and_merge(readings, rows, conn)
    })
    .await
}
