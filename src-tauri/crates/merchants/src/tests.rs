use crate::{
    create_merchant, create_merchant_by_name, delete_merchant, find_merchant_by_name,
    list_merchants as list_merchants_domain, update_merchant,
};
use ledger_infra::error::{AppError, ErrClass};

use super::model::{Merchant, MerchantInput, MerchantUpdateInput};

fn setup() -> rusqlite::Connection {
    tauri_app_lib::test_support::open()
}

fn list_merchants(conn: &rusqlite::Connection) -> Vec<Merchant> {
    list_merchants_domain(conn, false).unwrap()
}

#[test]
fn list_merchants_starts_empty() {
    let conn = setup();
    assert!(list_merchants(&conn).is_empty());
}

#[test]
fn create_merchant_returns_id_and_lists_sorted_by_name() {
    let conn = setup();
    let id_b = create_merchant(
        &conn,
        MerchantInput {
            name: "拼多多".into(),
        },
    )
    .unwrap();
    let id_a = create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    assert_ne!(id_a, id_b);

    let merchants = list_merchants(&conn);
    assert_eq!(merchants.len(), 2);
    assert_eq!(merchants[0].id, id_a);
    assert_eq!(merchants[0].name, "京东");
    assert_eq!(merchants[1].id, id_b);
    assert!(!merchants[0].is_deleted);
}

#[test]
fn create_merchant_duplicate_name_rejected() {
    let conn = setup();
    create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    let err = create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "商户已存在: 京东");
}

#[test]
fn update_merchant_renames() {
    let conn = setup();
    let id = create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    update_merchant(
        &conn,
        &id,
        MerchantUpdateInput {
            name: Some("京东商城".into()),
        },
    )
    .unwrap();

    let merchants = list_merchants(&conn);
    assert_eq!(merchants.len(), 1);
    assert_eq!(merchants[0].name, "京东商城");
}

#[test]
fn update_merchant_rename_to_taken_name_rejected() {
    let conn = setup();
    create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    let id = create_merchant(
        &conn,
        MerchantInput {
            name: "拼多多".into(),
        },
    )
    .unwrap();
    let err = update_merchant(
        &conn,
        &id,
        MerchantUpdateInput {
            name: Some("京东".into()),
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "商户已存在: 京东");
}

#[test]
fn update_merchant_trims_name() {
    let conn = setup();
    let id = create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    update_merchant(
        &conn,
        &id,
        MerchantUpdateInput {
            name: Some("  京东商城\t".into()),
        },
    )
    .unwrap();
    assert_eq!(list_merchants(&conn)[0].name, "京东商城");
}

#[test]
fn update_merchant_blank_name_rejected() {
    let conn = setup();
    let id = create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    let err = update_merchant(
        &conn,
        &id,
        MerchantUpdateInput {
            name: Some("   ".into()),
        },
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::Coded { ref message, .. } if message.contains("商户名不能为空"))
    );
    assert_eq!(list_merchants(&conn)[0].name, "京东");
}

#[test]
fn update_merchant_trim_collides_with_existing_name() {
    let conn = setup();
    create_merchant(
        &conn,
        MerchantInput {
            name: "美团".into(),
        },
    )
    .unwrap();
    let id = create_merchant(
        &conn,
        MerchantInput {
            name: "拼多多".into(),
        },
    )
    .unwrap();
    let err = update_merchant(
        &conn,
        &id,
        MerchantUpdateInput {
            name: Some("  美团  ".into()),
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "商户已存在: 美团");
}

#[test]
fn update_merchant_missing_is_not_found() {
    let conn = setup();
    let err = update_merchant(
        &conn,
        "no-such-id",
        MerchantUpdateInput {
            name: Some("新名".into()),
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
}

#[test]
fn delete_merchant_soft_deletes_and_hides_from_list() {
    let conn = setup();
    let id = create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    delete_merchant(&conn, &id).unwrap();
    assert!(list_merchants(&conn).is_empty());

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
    let err = delete_merchant(&conn, "no-such-id").unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
}

#[test]
fn recreate_same_name_after_soft_delete() {
    let conn = setup();
    let id = create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    delete_merchant(&conn, &id).unwrap();
    let new_id = create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    assert_ne!(new_id, id);
    let merchants = list_merchants(&conn);
    assert_eq!(merchants.len(), 1);
    assert_eq!(merchants[0].id, new_id);
}

#[test]
fn find_merchant_by_name_hits_active_exact_match() {
    let conn = setup();
    assert_eq!(find_merchant_by_name(&conn, "盒马").unwrap(), None);
    let id = create_merchant_by_name(&conn, "盒马").unwrap();
    assert_eq!(find_merchant_by_name(&conn, "盒马").unwrap(), Some(id));
}

#[test]
fn find_merchant_by_name_trims_and_blank_is_none() {
    let conn = setup();
    let id = create_merchant_by_name(&conn, "京东").unwrap();
    assert_eq!(find_merchant_by_name(&conn, "  京东\t").unwrap(), Some(id));
    assert_eq!(find_merchant_by_name(&conn, "   ").unwrap(), None);
    assert!(list_merchants(&conn).len() == 1);
}

#[test]
fn create_merchant_by_name_creates_trimmed() {
    let conn = setup();
    let id = create_merchant_by_name(&conn, "  盒马 ").unwrap();
    let merchants = list_merchants(&conn);
    assert_eq!(merchants.len(), 1);
    assert_eq!(merchants[0].id, id);
    assert_eq!(merchants[0].name, "盒马");
}

#[test]
fn create_merchant_by_name_reuses_exact_match() {
    let conn = setup();
    let first = create_merchant_by_name(&conn, "京东").unwrap();
    let second = create_merchant_by_name(&conn, "京东").unwrap();
    assert_eq!(first, second);
    assert_eq!(list_merchants(&conn).len(), 1);
}

#[test]
fn create_merchant_by_name_blank_is_invalid() {
    let conn = setup();
    let err = create_merchant_by_name(&conn, "   ").unwrap_err();
    assert!(
        matches!(err, AppError::Coded { ref message, .. } if message.contains("商户名不能为空"))
    );
    assert!(list_merchants(&conn).is_empty());
}

#[test]
fn create_merchant_by_name_ignores_soft_deleted() {
    let conn = setup();
    let old_id = create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    delete_merchant(&conn, &old_id).unwrap();
    let new_id = create_merchant_by_name(&conn, "京东").unwrap();
    assert_ne!(new_id, old_id);
}

#[test]
fn merchant_json_contract_has_no_icon_or_color() {
    let conn = setup();
    create_merchant(
        &conn,
        MerchantInput {
            name: "京东".into(),
        },
    )
    .unwrap();
    let merchants = list_merchants(&conn);
    let json = serde_json::to_value(&merchants[0]).unwrap();
    assert!(json.get("icon").is_none(), "响应不应含 icon 字段");
    assert!(json.get("color").is_none(), "响应不应含 color 字段");
}
