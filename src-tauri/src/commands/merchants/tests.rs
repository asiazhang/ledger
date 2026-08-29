//! 商户字典命令核心逻辑测试（issue #188 / ADR-0028）。

use crate::commands::merchants::{
    create_merchant_internal, delete_merchant_internal, list_merchants_internal,
    update_merchant_internal,
};
use crate::error::AppError;
use crate::models::{Merchant, MerchantInput, MerchantUpdateInput};

fn setup() -> rusqlite::Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn list_merchants(conn: &rusqlite::Connection) -> Vec<Merchant> {
    list_merchants_internal(conn, false).unwrap()
}

#[test]
fn list_merchants_starts_empty() {
    let conn = setup();
    // 空表启动不 seed（ADR-0028：商户是强个人属性，seed 是字典噪音）。
    assert!(list_merchants(&conn).is_empty());
}

#[test]
fn create_merchant_returns_id_and_lists_sorted_by_name() {
    let conn = setup();
    let id_b = create_merchant_internal(
        &conn,
        MerchantInput {
            name: "拼多多".into(),
            icon: None,
            color: Some("#ff0000".into()),
        },
    )
    .unwrap();
    let id_a = create_merchant_internal(
        &conn,
        MerchantInput {
            name: "京东".into(),
            icon: Some("cart".into()),
            color: None,
        },
    )
    .unwrap();
    assert_ne!(id_a, id_b);

    let merchants = list_merchants(&conn);
    assert_eq!(merchants.len(), 2);
    // 按名称排序（字典语义），而非创建顺序。
    assert_eq!(merchants[0].id, id_a);
    assert_eq!(merchants[0].name, "京东");
    assert_eq!(merchants[0].icon.as_deref(), Some("cart"));
    assert_eq!(merchants[1].id, id_b);
    assert_eq!(merchants[1].color.as_deref(), Some("#ff0000"));
    assert!(!merchants[0].is_deleted);
}

#[test]
fn create_merchant_duplicate_name_rejected() {
    let conn = setup();
    create_merchant_internal(
        &conn,
        MerchantInput {
            name: "京东".into(),
            icon: None,
            color: None,
        },
    )
    .unwrap();
    let err = create_merchant_internal(
        &conn,
        MerchantInput {
            name: "京东".into(),
            icon: None,
            color: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "参数错误: 商户已存在: 京东");
}

#[test]
fn update_merchant_renames_and_keeps_other_fields() {
    let conn = setup();
    let id = create_merchant_internal(
        &conn,
        MerchantInput {
            name: "京东".into(),
            icon: Some("cart".into()),
            color: Some("#e1251b".into()),
        },
    )
    .unwrap();
    // 只改名：icon/color 保持原值。
    update_merchant_internal(
        &conn,
        &id,
        MerchantUpdateInput {
            name: Some("京东商城".into()),
            icon: None,
            color: None,
        },
    )
    .unwrap();

    let merchants = list_merchants(&conn);
    assert_eq!(merchants.len(), 1);
    assert_eq!(merchants[0].name, "京东商城");
    assert_eq!(merchants[0].icon.as_deref(), Some("cart"));
    assert_eq!(merchants[0].color.as_deref(), Some("#e1251b"));
    // 改名即时生效：历史交易以 merchant_id 引用，不回刷交易行（本层只保证名字变更）。
}

#[test]
fn update_merchant_rename_to_taken_name_rejected() {
    let conn = setup();
    create_merchant_internal(
        &conn,
        MerchantInput {
            name: "京东".into(),
            icon: None,
            color: None,
        },
    )
    .unwrap();
    let id = create_merchant_internal(
        &conn,
        MerchantInput {
            name: "拼多多".into(),
            icon: None,
            color: None,
        },
    )
    .unwrap();
    let err = update_merchant_internal(
        &conn,
        &id,
        MerchantUpdateInput {
            name: Some("京东".into()),
            icon: None,
            color: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "参数错误: 商户已存在: 京东");
}

#[test]
fn update_merchant_missing_is_not_found() {
    let conn = setup();
    let err = update_merchant_internal(
        &conn,
        "no-such-id",
        MerchantUpdateInput {
            name: Some("新名".into()),
            icon: None,
            color: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)));
}

#[test]
fn delete_merchant_soft_deletes_and_hides_from_list() {
    let conn = setup();
    let id = create_merchant_internal(
        &conn,
        MerchantInput {
            name: "京东".into(),
            icon: None,
            color: None,
        },
    )
    .unwrap();
    delete_merchant_internal(&conn, &id).unwrap();
    // 列表不再包含（软删商户不可再被新交易选择）。
    assert!(list_merchants(&conn).is_empty());

    // 行仍在库中（历史引用保留，读回照常显示商户名），is_deleted=1。
    let (is_deleted, name): (i64, String) = conn
        .query_row(
            "SELECT is_deleted, name FROM merchants WHERE id=?1",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(is_deleted, 1);
    assert_eq!(name, "京东");
}

#[test]
fn delete_merchant_missing_is_not_found() {
    let conn = setup();
    let err = delete_merchant_internal(&conn, "no-such-id").unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)));
}

/// 软删后同名可重建：在用行唯一（partial unique index），软删行不占名字
/// （AI 导入未命中即建的前提，ADR-0028 代价 #2「重新录入或导入时自然重建」）。
#[test]
fn recreate_same_name_after_soft_delete() {
    let conn = setup();
    let id = create_merchant_internal(
        &conn,
        MerchantInput {
            name: "京东".into(),
            icon: None,
            color: None,
        },
    )
    .unwrap();
    delete_merchant_internal(&conn, &id).unwrap();
    let new_id = create_merchant_internal(
        &conn,
        MerchantInput {
            name: "京东".into(),
            icon: None,
            color: None,
        },
    )
    .unwrap();
    assert_ne!(new_id, id);
    let merchants = list_merchants(&conn);
    assert_eq!(merchants.len(), 1);
    assert_eq!(merchants[0].id, new_id);
}
