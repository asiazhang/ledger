use tauri::State;

use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Category, CategoryInput};

#[tauri::command]
pub fn list_categories(db: State<'_, DbState>) -> Result<Vec<Category>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT id,name,kind,parent_id,icon,color,created_at,updated_at,version,device_id,is_deleted \
         FROM categories WHERE is_deleted=0 ORDER BY kind, created_at",
        [],
    )
}

#[tauri::command]
pub fn create_category(db: State<'_, DbState>, input: CategoryInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,color,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,NULL,NULL,?5,?6,?7,?8,0)",
        rusqlite::params![id, input.name, input.kind, input.parent_id, now, now, 1, device_id()],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn delete_category(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "UPDATE categories SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}
