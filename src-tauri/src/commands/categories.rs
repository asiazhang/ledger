use rusqlite::Connection;
use tauri::State;

use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};

pub fn list_categories_internal(conn: &Connection) -> Result<Vec<Category>> {
    query_all(
        conn,
        "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
         FROM categories WHERE is_deleted=0 ORDER BY kind, sort_order, created_at",
        [],
    )
}

pub fn create_category_internal(conn: &Connection, input: CategoryInput) -> Result<String> {
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0)",
        rusqlite::params![
            id,
            input.name,
            input.kind,
            input.parent_id,
            input.icon,
            0,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

/// 按自然键（name + kind + parent_id）幂等创建分类：已存在（未删除）时返回已有 id，
/// 不重复插入、不报错。供 HTTP 导入 API 使用。
pub fn create_category_idempotent_internal(
    conn: &Connection,
    input: CategoryInput,
) -> Result<String> {
    if let Some(id) = find_category_by_natural_key(conn, &input)? {
        return Ok(id);
    }
    create_category_internal(conn, input)
}

fn find_category_by_natural_key(
    conn: &Connection,
    input: &CategoryInput,
) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM categories \
         WHERE name=?1 AND kind=?2 AND parent_id IS ?3 AND is_deleted=0 LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![input.name, input.kind, input.parent_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn list_categories(db: State<'_, DbState>) -> Result<Vec<Category>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    list_categories_internal(&conn)
}

#[tauri::command]
pub fn create_category(db: State<'_, DbState>, input: CategoryInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    create_category_internal(&conn, input)
}

#[tauri::command]
pub fn update_category(
    db: State<'_, DbState>,
    id: String,
    input: CategoryUpdateInput,
) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let now = now_iso();
    let did = device_id();

    let existing: Category = query_all(
        &conn,
        "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
         FROM categories WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| AppError::NotFound(format!("分类不存在: {id}")))?;

    let parent_id = input.parent_id.unwrap_or(existing.parent_id);

    if let Some(ref pid) = parent_id {
        if *pid == id {
            return Err(AppError::Invalid("自身不能作为父分类".into()));
        }
        let parent: Category = query_all(
            &conn,
            "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
             FROM categories WHERE id=?1 AND is_deleted=0",
            rusqlite::params![pid],
        )?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound(format!("父分类不存在: {pid}")))?;
        if parent.kind != existing.kind {
            return Err(AppError::Invalid("父分类类型需一致".into()));
        }
    }

    let name = input.name.unwrap_or(existing.name);
    let icon = input.icon.or(existing.icon);

    conn.execute(
        "UPDATE categories SET name=?1, icon=?2, parent_id=?3, updated_at=?4, version=version+1, device_id=?5 WHERE id=?6",
        rusqlite::params![name, icon, parent_id, now, did, id],
    )?;
    Ok(())
}

#[tauri::command]
pub fn reorder_categories(db: State<'_, DbState>, items: Vec<ReorderItem>) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let now = now_iso();
    let did = device_id();
    for item in &items {
        conn.execute(
            "UPDATE categories SET sort_order=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![item.sort_order, now, did, item.id],
        )?;
    }
    Ok(())
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
            "SELECT id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted \
             FROM categories WHERE is_deleted=0 ORDER BY kind, sort_order, created_at",
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
            "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,NULL,NULL,0,?4,?5,?6,?7,0)",
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
            "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'expense',NULL,NULL,0,?3,?4,?5,?6,0)",
            rusqlite::params![parent_id, "出行", now, now, 1, device_id()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'expense',?3,NULL,0,?4,?5,?6,?7,0)",
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
            "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'expense',NULL,NULL,0,?3,?4,?5,?6,0)",
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

    #[test]
    fn update_category_updates_fields() {
        use crate::models::CategoryUpdateInput;
        let conn = setup();
        let id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'expense',NULL,NULL,0,?3,?4,?5,?6,0)",
            rusqlite::params![id, "原始分类", now, now, 1, device_id()],
        )
        .unwrap();

        let input = CategoryUpdateInput {
            name: Some("更新后".into()),
            icon: Some("🍕".into()),
            parent_id: None,
        };
        conn.execute(
            "UPDATE categories SET name=?1, icon=?2, parent_id=?3, updated_at=?4, version=version+1, device_id=?5 WHERE id=?6",
            rusqlite::params![input.name, input.icon, input.parent_id.unwrap_or(None), now_iso(), device_id(), id],
        )
        .unwrap();
        let cats = list_categories(&conn);
        let updated = cats.iter().find(|c| c.id == id).unwrap();
        assert_eq!(updated.name, "更新后");
        assert_eq!(updated.icon.as_deref(), Some("🍕"));
    }

    #[test]
    fn reorder_categories_sets_sort_order() {
        let conn = setup();
        let id1 = new_uuid();
        let id2 = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'expense',NULL,NULL,0,?3,?4,?5,?6,0)",
            rusqlite::params![id1, "分类A", now, now, 1, device_id()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'expense',NULL,NULL,0,?3,?4,?5,?6,0)",
            rusqlite::params![id2, "分类B", now, now, 1, device_id()],
        )
        .unwrap();

        conn.execute(
            "UPDATE categories SET sort_order=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![2, now_iso(), device_id(), id1],
        )
        .unwrap();
        conn.execute(
            "UPDATE categories SET sort_order=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![1, now_iso(), device_id(), id2],
        )
        .unwrap();

        let cats = list_categories(&conn);
        let a = cats.iter().find(|c| c.id == id1).unwrap();
        let b = cats.iter().find(|c| c.id == id2).unwrap();
        assert_eq!(b.sort_order, 1);
        assert_eq!(a.sort_order, 2);
        let a_pos = cats.iter().position(|c| c.id == id1).unwrap();
        let b_pos = cats.iter().position(|c| c.id == id2).unwrap();
        assert!(b_pos < a_pos);
    }
}
