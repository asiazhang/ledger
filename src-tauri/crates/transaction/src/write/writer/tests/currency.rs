use ledger_infra::error::AppError;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::writer::{
    Input, NormalizedRow, insert_row, normalize,
};

use super::common::input;
use tauri_app_lib::test_support;

fn assert_currency_mismatch(err: AppError, expected: [&str; 2]) {
    match err {
        AppError::Coded { code, params, .. } => {
            assert_eq!(code, "transaction.currency-mismatch");
            assert_eq!(
                params,
                expected.map(String::from).to_vec(),
                "params 契约 = [账户币种, 交易币种]"
            );
        }
        other => panic!("应为 transaction.currency-mismatch 码化错误，实际: {other:?}"),
    }
}

#[test]
fn normalize_rejects_currency_mismatching_single_account() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    for kind in [TransactionKind::Income, TransactionKind::Expense] {
        let err = normalize(
            &conn,
            &Input {
                currency_code: "CNY".into(),
                ..input(kind, 1000, "acc-usd")
            },
        )
        .unwrap_err();
        assert_currency_mismatch(err, ["USD", "CNY"]);
    }
}

#[test]
fn normalize_allows_matching_non_default_currency() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_fx_rate_history(&conn, "fxh-w", "USD", "CNY", "2025-12-29", 7.2);
    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap();
    assert_eq!(norm.currency_code, "USD");
    assert_eq!(norm.amount_native_cents, 72000);
}

#[test]
fn normalize_rejects_transfer_to_end_currency_mismatch() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    let err = normalize(
        &conn,
        &Input {
            currency_code: "CNY".into(),
            to_account_id: Some("acc-usd".into()),
            ..input(TransactionKind::Transfer, 3000, "acc-cny")
        },
    )
    .unwrap_err();
    assert_currency_mismatch(err, ["USD", "CNY"]);
}

#[test]
fn normalize_allows_transfer_with_both_ends_matching() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd-a", "acc-usd-a", "cash", "USD", 0);
    test_support::seed_account(&conn, "acc-usd-b", "acc-usd-b", "cash", "USD", 0);
    test_support::seed_fx_rate_history(&conn, "fxh-w", "USD", "CNY", "2025-12-29", 7.2);
    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            to_account_id: Some("acc-usd-b".into()),
            ..input(TransactionKind::Transfer, 3000, "acc-usd-a")
        },
    )
    .unwrap();
    assert_eq!(norm.to_account_id.as_deref(), Some("acc-usd-b"));
}

#[test]
fn normalize_skips_guard_when_account_row_unreadable() {
    let conn = test_support::open();
    let norm = normalize(&conn, &input(TransactionKind::Income, 1000, "no-such-acc")).unwrap();
    assert_eq!(norm.account_id.as_deref(), Some("no-such-acc"));
}

#[test]
fn normalize_refund_guard_runs_on_inherited_dirty_source() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    let dirty = NormalizedRow {
        funding: Vec::new(),
        kind: TransactionKind::Expense,
        amount_cents: 1000,
        currency_code: "USD".into(),
        amount_native_cents: 1000,
        fx_rate_used: None,
        fx_rate_source: None,
        account_id: Some("acc-cny".into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        policy_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-01-01".into(),
        purchases: Vec::new(),
    };
    let source_id = insert_row(&conn, &dirty).unwrap();

    let err = normalize(
        &conn,
        &Input {
            kind: TransactionKind::Refund,
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 200, "acc-cny")
        },
    )
    .unwrap_err();
    assert_currency_mismatch(err, ["CNY", "USD"]);
}
