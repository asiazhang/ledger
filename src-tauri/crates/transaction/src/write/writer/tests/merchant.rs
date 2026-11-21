use rusqlite::{Connection, params};

use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::writer::{Input, insert_row, normalize};

use super::common::{input, insert_source_expense};
use tauri_app_lib::test_support;

fn insert_merchant(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?3,1,'test',0)",
        params![id, name, test_support::FIXED_NOW],
    )
    .unwrap();
}

#[test]
fn normalize_merchant_passthrough() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");
    let norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-jd".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id.as_deref(), Some("mer-jd"));
}

#[test]
fn normalize_merchant_not_found_is_rejected() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &Input {
            merchant_id: Some("no-such-merchant".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "商户不存在或已删除: no-such-merchant");
}

#[test]
fn normalize_soft_deleted_merchant_is_rejected_for_new_txn() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-dead", "已删商户");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-dead'", [])
        .unwrap();
    let err = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-dead".into()),
            ..input(TransactionKind::Income, 1000, "acc")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("商户不存在或已删除"));
}

#[test]
fn normalize_refund_inherits_source_merchant() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-src", "acc-src", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");
    insert_merchant(&conn, "mer-pdd", "拼多多");
    let source_norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-jd".into()),
            ..input(TransactionKind::Expense, 1000, "acc-src")
        },
    )
    .unwrap();
    let source_id = insert_row(&conn, &source_norm).unwrap();

    let norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-pdd".into()),
            refund_of_transaction_id: Some(source_id.clone()),
            ..input(TransactionKind::Refund, 200, "acc-src")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id.as_deref(), Some("mer-jd"));
    assert_eq!(
        norm.refund_of_transaction_id.as_deref(),
        Some(source_id.as_str())
    );
}

#[test]
fn normalize_refund_without_source_merchant_has_none() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-src", "acc-src", "cash", "CNY", 0);
    let source_id = insert_source_expense(&conn, "acc-src", None);
    let norm = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 200, "acc-src")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id, None);
}

#[test]
fn normalize_keeps_unchanged_merchant_even_if_soft_deleted() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-dead", "已删商户");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-dead'", [])
        .unwrap();
    let norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-dead".into()),
            existing_merchant_id: Some("mer-dead".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id.as_deref(), Some("mer-dead"));
}

#[test]
fn normalize_rejects_changing_to_soft_deleted_merchant() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-old", "旧商户");
    insert_merchant(&conn, "mer-dead", "已删商户");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-dead'", [])
        .unwrap();
    let err = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-dead".into()),
            existing_merchant_id: Some("mer-old".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("商户不存在或已删除"));
}
