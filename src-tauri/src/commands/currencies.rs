use tauri::State;

use crate::db::DbState;
use crate::error::Result;
use crate::models::Currency;

#[tauri::command]
pub fn list_currencies(db: State<'_, DbState>) -> Result<Vec<Currency>> {
    let conn = db
        .conn
        .lock()
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let mut stmt =
        conn.prepare("SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code")?;
    let rows = stmt.query_map([], |r| {
        Ok(Currency {
            code: r.get(0)?,
            name: r.get(1)?,
            symbol: r.get(2)?,
            decimal_places: r.get(3)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}
