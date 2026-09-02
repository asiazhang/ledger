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
fn delete_category_soft_deletes_and_excludes_from_readback() {
    let conn = setup();
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'expense',NULL,NULL,0,?3,?4,?5,?6,0)",
        rusqlite::params![id, "临时分类", now, now, 1, device_id()],
    )
    .unwrap();
    super::delete_category(&conn, &id).unwrap();
    assert!(
        !list_categories(&conn).iter().any(|c| c.id == id),
        "删除后不应出现在读回结果中"
    );
}

#[test]
fn delete_category_returns_not_found_for_missing_id() {
    let conn = setup();
    let err = super::delete_category(&conn, "不存在的id").unwrap_err();
    assert!(matches!(
        err,
        crate::error::AppError::Coded {
            class: crate::error::ErrClass::NotFound,
            ..
        }
    ));
    assert!(err.to_string().contains("分类不存在"));
}

#[test]
fn delete_category_returns_not_found_for_already_deleted() {
    let conn = setup();
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'expense',NULL,NULL,0,?3,?4,?5,?6,0)",
        rusqlite::params![id, "临时分类", now, now, 1, device_id()],
    )
    .unwrap();
    super::delete_category(&conn, &id).unwrap();
    let err = super::delete_category(&conn, &id).unwrap_err();
    assert!(
        matches!(
            err,
            crate::error::AppError::Coded {
                class: crate::error::ErrClass::NotFound,
                ..
            }
        ),
        "已删除分类应再次返回 404"
    );
}

// ----- 预算删除守卫（issue #355）-----

fn insert_expense_category_row(
    conn: &rusqlite::Connection,
    name: &str,
    parent_id: Option<&str>,
) -> String {
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'expense',?3,NULL,0,?4,?5,?6,?7,0)",
        rusqlite::params![id, name, parent_id, now, now, 1, device_id()],
    )
    .unwrap();
    id
}

fn insert_budget_row(conn: &rusqlite::Connection, category_id: &str, is_deleted: i64) {
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'monthly',50000,'2026-01-01',?3,?3,1,?4,?5)",
        rusqlite::params![new_uuid(), category_id, now_iso(), device_id(), is_deleted],
    )
    .unwrap();
}

#[test]
fn delete_category_rejects_when_undeleted_budget_exists() {
    let conn = setup();
    let id = insert_expense_category_row(&conn, "带预算分类", None);
    insert_budget_row(&conn, &id, 0);
    let err = super::delete_category(&conn, &id).unwrap_err();
    match err {
        crate::error::AppError::Coded { code, message, .. } => {
            assert_eq!(code, "category.has-budget");
            assert!(
                message.contains("请先删除对应预算"),
                "应引导先删预算: {message}"
            );
        }
        other => panic!("应为码化错误，实际 {other:?}"),
    }
    assert!(
        list_categories(&conn).iter().any(|c| c.id == id),
        "被拒后分类不应被删除"
    );
}

#[test]
fn delete_category_allows_when_only_soft_deleted_budgets() {
    let conn = setup();
    let id = insert_expense_category_row(&conn, "软删预算分类", None);
    insert_budget_row(&conn, &id, 1);
    super::delete_category(&conn, &id).unwrap();
    assert!(
        !list_categories(&conn).iter().any(|c| c.id == id),
        "仅剩软删除预算时分类应可正常删除"
    );
}

#[test]
fn delete_category_ignores_budgets_of_subcategories() {
    let conn = setup();
    let parent = insert_expense_category_row(&conn, "预算父分类", None);
    let child = insert_expense_category_row(&conn, "预算子分类", Some(&parent));
    insert_budget_row(&conn, &child, 0);
    super::delete_category(&conn, &parent).unwrap();
    assert!(
        !list_categories(&conn).iter().any(|c| c.id == parent),
        "父分类应被删除"
    );
    assert!(
        list_categories(&conn).iter().any(|c| c.id == child),
        "子分类不应受牵连"
    );
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
