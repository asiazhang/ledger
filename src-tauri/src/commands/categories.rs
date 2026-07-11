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

#[cfg(test)]
mod tests {
    use crate::db::query::query_all;
    use crate::db::{device_id, new_uuid, now_iso};
    use crate::models::Category;

    fn setup() -> rusqlite::Connection {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn list_categories(conn: &rusqlite::Connection) -> Vec<Category> {
        query_all(
            conn,
            "SELECT id,name,kind,parent_id,icon,color,created_at,updated_at,version,device_id,is_deleted \
             FROM categories WHERE is_deleted=0 ORDER BY kind, created_at",
            [],
        )
        .unwrap()
    }

    #[test]
    fn list_categories_returns_seed_data() {
        let conn = setup();
        let cats = list_categories(&conn);
        assert!(cats.len() >= 92);
        let expense_count = cats.iter().filter(|c| c.kind == "expense").count();
        let income_count = cats.iter().filter(|c| c.kind == "income").count();
        assert!(expense_count > 0);
        assert!(income_count > 0);
    }

    #[test]
    fn create_category_inserts_and_returns_id() {
        let conn = setup();
        let id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,color,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,NULL,NULL,NULL,?4,?5,?6,?7,0)",
            rusqlite::params![id, "交通", "expense", now, now, 1, device_id()],
        )
        .unwrap();
        let cats = list_categories(&conn);
        assert!(cats.iter().any(|c| c.id == id && c.name == "交通"));
    }

    #[test]
    fn create_subcategory_with_parent() {
        let conn = setup();
        let parent_id = new_uuid();
        let child_id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,color,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'expense',NULL,NULL,NULL,?3,?4,?5,?6,0)",
            rusqlite::params![parent_id, "出行", now, now, 1, device_id()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,color,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'expense',?3,NULL,NULL,?4,?5,?6,?7,0)",
            rusqlite::params![child_id, "打车", parent_id, now, now, 1, device_id()],
        )
        .unwrap();
        let cats = list_categories(&conn);
        let child = cats.iter().find(|c| c.id == child_id).unwrap();
        assert_eq!(child.parent_id.as_deref(), Some(&*parent_id));
    }

    #[test]
    fn delete_category_soft_deletes() {
        let conn = setup();
        let id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,color,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'expense',NULL,NULL,NULL,?3,?4,?5,?6,0)",
            rusqlite::params![id, "临时分类", now, now, 1, device_id()],
        )
        .unwrap();
        assert!(list_categories(&conn).iter().any(|c| c.id == id));
        conn.execute(
            "UPDATE categories SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
            rusqlite::params![id, now_iso(), device_id()],
        )
        .unwrap();
        assert!(!list_categories(&conn).iter().any(|c| c.id == id));
    }
}
