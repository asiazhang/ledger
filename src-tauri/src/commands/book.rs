#![allow(clippy::unreachable)]

use tauri::{AppHandle, Runtime};

use crate::commands::boot::current_boot;
use crate::commands::data_location::default_data_dir;
use ledger_infra::db::book_registry::{self, BookListInfo};
use ledger_infra::db::data_location;
use ledger_infra::db::run_db;
use ledger_infra::error::Result;

#[tauri::command]
pub async fn list_books<R: Runtime>(app: AppHandle<R>) -> Result<BookListInfo> {
    run_db("list_books", move || {
        let default_dir = default_data_dir(&app)?;
        Ok(data_location::gather_book_list(
            &default_dir,
            current_boot(&app).as_ref(),
        ))
    })
    .await
}

#[tauri::command]
pub async fn create_book<R: Runtime>(
    app: AppHandle<R>,
    name: String,
) -> Result<book_registry::Book> {
    run_db("create_book", move || {
        let default_dir = default_data_dir(&app)?;
        data_location::mutable_registry(current_boot(&app).as_ref())?;
        book_registry::create_book_entry(&default_dir, &name)
    })
    .await
}

#[tauri::command]
pub async fn switch_book<R: Runtime>(app: AppHandle<R>, id: String) -> Result<book_registry::Book> {
    run_db("switch_book", move || {
        let default_dir = default_data_dir(&app)?;
        data_location::mutable_registry(current_boot(&app).as_ref())?;
        book_registry::switch_active_book(&default_dir, &id)
    })
    .await
}

#[tauri::command]
pub async fn rename_book<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    name: String,
) -> Result<book_registry::Book> {
    run_db("rename_book", move || {
        let default_dir = default_data_dir(&app)?;
        data_location::mutable_registry(current_boot(&app).as_ref())?;
        book_registry::rename_book_entry(&default_dir, &id, &name)
    })
    .await
}

#[tauri::command]
pub async fn remove_book<R: Runtime>(app: AppHandle<R>, id: String) -> Result<()> {
    run_db("remove_book", move || {
        let default_dir = default_data_dir(&app)?;
        data_location::mutable_registry(current_boot(&app).as_ref())?;
        book_registry::remove_book_entry(&default_dir, &id)
    })
    .await
}
