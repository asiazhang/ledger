use tauri::State;

use crate::db::DbState;
use crate::db::query::query_all;
use crate::error::Result;
use crate::models::Currency;

#[tauri::command]
pub fn list_currencies(db: State<'_, DbState>) -> Result<Vec<Currency>> {
    let conn = db
        .conn
        .lock()
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code",
        [],
    )
}
