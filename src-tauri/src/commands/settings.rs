#![allow(clippy::unreachable)]

use serde::Serialize;
use tauri::Manager;

use crate::shell_support::logger;
use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_currencies::current_base_currency;
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;

#[derive(Debug, Serialize)]
pub struct LogLevelState {
    pub level: String,
}

#[tauri::command]
pub async fn get_log_level(app: tauri::AppHandle) -> Result<LogLevelState> {
    let conn = app.state::<DbState>().read_handle();
    read_entry("get_log_level", conn, move |conn| {
        let level = logger::persisted_level(conn);
        Ok(LogLevelState {
            level: level.directive().to_string(),
        })
    })
    .await
}

#[tauri::command]
pub async fn set_log_level(app: tauri::AppHandle, level: String) -> Result<()> {
    let db = app.state::<DbState>().write_handle();
    db.run_raw("set_log_level", move |conn| {
        logger::set_persisted_level(conn, &level)
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct BaseCurrencyState {
    pub code: String,
}

#[tauri::command]
pub async fn get_base_currency(app: tauri::AppHandle) -> Result<BaseCurrencyState> {
    let conn = app.state::<DbState>().read_handle();
    read_entry("get_base_currency", conn, move |conn| {
        Ok(BaseCurrencyState {
            code: current_base_currency(conn)?,
        })
    })
    .await
}

#[tauri::command]
pub async fn set_base_currency(app: tauri::AppHandle, code: String) -> Result<BaseCurrencyState> {
    let conn = app.state::<DbState>().write_handle();
    write_entry(
        "set_base_currency",
        conn,
        Some(&app),
        WriteOp::SetBaseCurrency,
        move |conn| {
            ledger_currencies::set_base_currency(conn, &code)?;
            Ok(Outcome::Silent(BaseCurrencyState { code }))
        },
    )
    .await
}
