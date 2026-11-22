use rusqlite::params;

use ledger_infra::error::{AppError, ErrClass};
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::writer::{Input, insert_row, normalize};

use super::common::{input, insert_source_expense};
use crate::tests::common::seed_category;
use tauri_app_lib::test_support;

#[test]
fn normalize_refund_requires_source_id() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let err = normalize(&conn, &input(TransactionKind::Refund, 200, "acc")).unwrap_err();
    assert_eq!(err.to_string(), "退款必须关联原支出交易");
}

#[test]
fn normalize_refund_inherits_source_fields() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-src", "acc-src", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-other", "acc-other", "cash", "USD", 0);
    let cat_src = seed_category(&conn, "餐饮", "expense");
    let source_id = insert_source_expense(&conn, "acc-src", Some(&cat_src));

    let norm = normalize(
        &conn,
        &Input {
            kind: TransactionKind::Refund,
            amount_cents: 200,
            currency_code: "USD".into(),
            account_id: Some("acc-other".into()),
            category_id: Some("cat-other".into()),
            refund_of_transaction_id: Some(source_id.clone()),
            ..input(TransactionKind::Refund, 200, "acc-other")
        },
    )
    .unwrap();
    assert_eq!(norm.account_id.as_deref(), Some("acc-src"));
    assert_eq!(norm.currency_code, "CNY");
    assert_eq!(norm.category_id.as_deref(), Some(cat_src.as_str()));
    assert_eq!(
        norm.refund_of_transaction_id.as_deref(),
        Some(source_id.as_str())
    );
    assert_eq!(norm.amount_cents, 200);
    assert_eq!(norm.date, "2026-01-01");
}

#[test]
fn normalize_refund_rejects_non_expense_source() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let income_norm = normalize(&conn, &input(TransactionKind::Income, 1000, "acc")).unwrap();
    let income_id = insert_row(&conn, &income_norm).unwrap();

    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(income_id),
            ..input(TransactionKind::Refund, 200, "acc")
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "退款只能关联支出交易");
}

#[test]
fn normalize_refund_source_not_found() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some("no-such-id".into()),
            ..input(TransactionKind::Refund, 200, "acc")
        },
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "应返回 NotFound，实际: {err:?}"
    );
}

#[test]
fn normalize_refund_source_soft_deleted_is_not_found() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let source_id = insert_source_expense(&conn, "acc", None);
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![source_id],
    )
    .unwrap();

    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 200, "acc")
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
