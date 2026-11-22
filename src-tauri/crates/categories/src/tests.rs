use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, ErrClass};
use ledger_sync_protocol::device::device_id;

use super::model::{Category, CategoryInput, ReorderItem};

fn setup() -> rusqlite::Connection {
    tauri_app_lib::test_support::open()
}

fn list_categories(conn: &rusqlite::Connection) -> Vec<Category> {
    super::list_categories(conn, false).unwrap()
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
    let id = super::create_category(
        &conn,
        CategoryInput {
            name: "交通".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    let cats = list_categories(&conn);
    let created = cats
        .iter()
        .find(|c| c.id == id)
        .expect("公开读回必须含新建行");
    assert_eq!(created.name, "交通");
    assert!(!created.is_deleted);
}

#[test]
fn create_subcategory_with_parent() {
    let conn = setup();
    let parent_id = super::create_category(
        &conn,
        CategoryInput {
            name: "出行".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    let child_id = super::create_category(
        &conn,
        CategoryInput {
            name: "打车".into(),
            kind: "expense".into(),
            parent_id: Some(parent_id.clone()),
            icon: None,
        },
    )
    .unwrap();
    let cats = list_categories(&conn);
    let child = cats.iter().find(|c| c.id == child_id).unwrap();
    assert_eq!(child.parent_id.as_deref(), Some(&*parent_id));
}

#[test]
fn create_category_idempotent_reuses_natural_key() {
    let conn = setup();
    let input = || CategoryInput {
        name: "餐饮".into(),
        kind: "expense".into(),
        parent_id: None,
        icon: None,
    };
    let first = super::create_category_idempotent(&conn, input()).unwrap();
    let second = super::create_category_idempotent(&conn, input()).unwrap();
    assert_eq!(first, second, "同自然键二次创建必须复用同一 id");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM categories WHERE name='餐饮' AND is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "幂等命中不得重复插入");
    let row_id: String = conn
        .query_row(
            "SELECT id FROM categories WHERE name='餐饮' AND is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(second, row_id);
}

#[test]
fn delete_category_soft_deletes_and_excludes_from_readback() {
    let conn = setup();
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'expense',NULL,NULL,0,?3,?4,?5,?6,0)",
        rusqlite::params![id, "临时分类", now, now, 1, device_id(&conn).unwrap()],
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
        AppError::Coded {
            class: ErrClass::NotFound,
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
        rusqlite::params![id, "临时分类", now, now, 1, device_id(&conn).unwrap()],
    )
    .unwrap();
    super::delete_category(&conn, &id).unwrap();
    let err = super::delete_category(&conn, &id).unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "已删除分类应再次返回 404"
    );
}

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
        rusqlite::params![id, name, parent_id, now, now, 1, device_id(conn).unwrap()],
    )
    .unwrap();
    id
}

fn insert_budget_row(conn: &rusqlite::Connection, category_id: &str, is_deleted: i64) {
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'monthly',50000,'2026-01-01',?3,?3,1,?4,?5)",
        rusqlite::params![new_uuid(), category_id, now_iso(), device_id(conn).unwrap(), is_deleted],
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
        AppError::Coded { code, message, .. } => {
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
    use super::model::CategoryUpdateInput;

    let conn = setup();
    let id = super::create_category(
        &conn,
        CategoryInput {
            name: "原始分类".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();

    super::update_category(
        &conn,
        &id,
        CategoryUpdateInput {
            name: Some("更新后".into()),
            icon: Some(Some("🍕".into())),
            parent_id: None,
        },
    )
    .unwrap();
    let cats = list_categories(&conn);
    let updated = cats.iter().find(|c| c.id == id).unwrap();
    assert_eq!(updated.name, "更新后");
    assert_eq!(updated.icon.as_deref(), Some("🍕"));
}

#[test]
fn category_update_input_distinguishes_null_from_missing_fields() {
    use super::model::CategoryUpdateInput;

    let cleared: CategoryUpdateInput = serde_json::from_str(r#"{"parent_id":null}"#).unwrap();
    assert_eq!(
        cleared.parent_id,
        Some(None),
        "显式 null = 清空（提升为顶级分类）"
    );
    let untouched: CategoryUpdateInput = serde_json::from_str("{}").unwrap();
    assert_eq!(untouched.parent_id, None, "键缺席 = 不改");
    let set: CategoryUpdateInput = serde_json::from_str(r#"{"parent_id":"cat-1"}"#).unwrap();
    assert_eq!(set.parent_id, Some(Some("cat-1".to_string())));

    let icon_cleared: CategoryUpdateInput = serde_json::from_str(r#"{"icon":null}"#).unwrap();
    assert_eq!(icon_cleared.icon, Some(None), "显式 null = 清空图标");
    let icon_untouched: CategoryUpdateInput = serde_json::from_str("{}").unwrap();
    assert_eq!(icon_untouched.icon, None, "键缺席 = 不改");
    let icon_set: CategoryUpdateInput = serde_json::from_str(r#"{"icon":"🍕"}"#).unwrap();
    assert_eq!(icon_set.icon, Some(Some("🍕".to_string())));
}

#[test]
fn update_category_clears_icon_with_explicit_none() {
    use super::model::{CategoryInput, CategoryUpdateInput};

    let conn = setup();
    let id = super::create_category(
        &conn,
        CategoryInput {
            name: "带图标".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: Some("🍕".into()),
        },
    )
    .unwrap();
    assert_eq!(icon_of(&conn, &id).as_deref(), Some("🍕"));

    super::update_category(
        &conn,
        &id,
        CategoryUpdateInput {
            name: None,
            icon: Some(None),
            parent_id: None,
        },
    )
    .unwrap();
    assert_eq!(icon_of(&conn, &id), None, "显式清空即无图标");

    super::update_category(
        &conn,
        &id,
        CategoryUpdateInput {
            name: None,
            icon: Some(Some("🍜".into())),
            parent_id: None,
        },
    )
    .unwrap();
    super::update_category(
        &conn,
        &id,
        CategoryUpdateInput {
            name: Some("带图标Ⅱ".into()),
            icon: None,
            parent_id: None,
        },
    )
    .unwrap();
    assert_eq!(icon_of(&conn, &id).as_deref(), Some("🍜"));
}

fn icon_of(conn: &rusqlite::Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT icon FROM categories WHERE id=?1",
        rusqlite::params![id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn update_category_promotes_child_to_top_level_with_explicit_none() {
    use super::model::{CategoryInput, CategoryUpdateInput};

    let conn = setup();
    let parent = super::create_category(
        &conn,
        CategoryInput {
            name: "父分类".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    let child = super::create_category(
        &conn,
        CategoryInput {
            name: "子分类".into(),
            kind: "expense".into(),
            parent_id: Some(parent.clone()),
            icon: None,
        },
    )
    .unwrap();
    assert_eq!(
        parent_id_of(&conn, &child).as_deref(),
        Some(parent.as_str())
    );

    super::update_category(
        &conn,
        &child,
        CategoryUpdateInput {
            name: None,
            icon: None,
            parent_id: Some(None),
        },
    )
    .unwrap();
    assert_eq!(parent_id_of(&conn, &child), None, "显式清空即顶级分类");

    super::update_category(
        &conn,
        &child,
        CategoryUpdateInput {
            name: None,
            icon: None,
            parent_id: Some(Some(parent.clone())),
        },
    )
    .unwrap();
    super::update_category(
        &conn,
        &child,
        CategoryUpdateInput {
            name: Some("子分类Ⅱ".into()),
            icon: None,
            parent_id: None,
        },
    )
    .unwrap();
    assert_eq!(
        parent_id_of(&conn, &child).as_deref(),
        Some(parent.as_str())
    );
}

fn parent_id_of(conn: &rusqlite::Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT parent_id FROM categories WHERE id=?1",
        rusqlite::params![id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn reorder_categories_sets_sort_order() {
    let conn = setup();
    let id1 = super::create_category(
        &conn,
        CategoryInput {
            name: "分类A".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    let id2 = super::create_category(
        &conn,
        CategoryInput {
            name: "分类B".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();

    super::reorder_categories(
        &conn,
        vec![
            ReorderItem {
                id: id2.clone(),
                sort_order: 1,
            },
            ReorderItem {
                id: id1.clone(),
                sort_order: 2,
            },
        ],
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
