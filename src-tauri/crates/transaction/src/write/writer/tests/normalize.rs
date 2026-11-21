use ledger_infra::error::AppError;
use tauri_app_lib::ledger_transaction::amount::{FxRateSource, TransactionKind};
use tauri_app_lib::ledger_transaction::write::writer::{Input, normalize};

use super::common::input;
use crate::tests::common::seed_category;
use tauri_app_lib::test_support;

#[test]
fn normalize_income_passthrough() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &Input {
            note: Some("工资".into()),
            ..input(TransactionKind::Income, 5000, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.kind, TransactionKind::Income);
    assert_eq!(norm.amount_cents, 5000);
    assert_eq!(norm.currency_code, "CNY");
    assert_eq!(norm.amount_native_cents, 5000, "本位币与原始币种应 1:1");
    assert_eq!(norm.account_id.as_deref(), Some("acc"));
    assert_eq!(norm.to_account_id, None);
    assert_eq!(norm.refund_of_transaction_id, None);
    assert_eq!(norm.note.as_deref(), Some("工资"));
    assert_eq!(norm.date, "2026-01-01");
}

#[test]
fn normalize_expense_passes_optional_fields() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let cat_food = seed_category(&conn, "餐饮", "expense");
    let norm = normalize(
        &conn,
        &Input {
            category_id: Some(cat_food.clone()),
            note: Some("午餐".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.category_id.as_deref(), Some(cat_food.as_str()));
    assert_eq!(norm.note.as_deref(), Some("午餐"));
}

#[test]
fn normalize_rejects_non_positive_amount() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    for bad in [0, -1, -500] {
        let err = normalize(&conn, &input(TransactionKind::Expense, bad, "acc")).unwrap_err();
        assert_eq!(err.to_string(), "金额必须大于 0", "金额 {bad}");
    }
}

#[test]
fn normalize_transfer_requires_to_account() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(&conn, &input(TransactionKind::Transfer, 3000, "acc-a")).unwrap_err();
    assert_eq!(err.to_string(), "转账必须指定目标账户");
}

#[test]
fn normalize_transfer_passes_to_account() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &Input {
            to_account_id: Some("acc-b".into()),
            ..input(TransactionKind::Transfer, 3000, "acc-a")
        },
    )
    .unwrap();
    assert_eq!(norm.account_id.as_deref(), Some("acc-a"));
    assert_eq!(norm.to_account_id.as_deref(), Some("acc-b"));
    assert_eq!(norm.refund_of_transaction_id, None);
}

#[test]
fn normalize_rejects_non_generic_kinds() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    for kind in [
        TransactionKind::Buy,
        TransactionKind::Sell,
        TransactionKind::Dividend,
        TransactionKind::Split,
    ] {
        let err = normalize(&conn, &input(kind, 1000, "acc")).unwrap_err();
        assert!(
            err.to_string().contains("仅处理通用交易类型"),
            "kind={kind:?} 应被拒绝，实际: {err}"
        );
    }
}

#[test]
fn normalize_converts_via_amount_seam_to_default_currency() {
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
    assert_eq!(norm.amount_cents, 10000);
    assert_eq!(norm.currency_code, "USD");
    assert_eq!(norm.amount_native_cents, 72000);
}

#[test]
fn normalize_explicit_fx_rate_skips_series_and_traces_explicit() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_fx_rate_history(&conn, "fxh-w", "USD", "CNY", "2025-12-29", 7.2);

    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            fx_rate: Some(7.0),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap();
    assert_eq!(
        norm.amount_native_cents, 70000,
        "native = amount × 显式汇率"
    );
    assert_eq!(norm.fx_rate_used, Some(7.0));
    assert_eq!(
        norm.fx_rate_source,
        Some(FxRateSource::Explicit),
        "留痕标为显式"
    );

    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap();
    assert_eq!(norm.amount_native_cents, 72000);
    assert_eq!(norm.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn normalize_explicit_fx_rate_non_positive_coded_error() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    let err = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            fx_rate: Some(0.0),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap_err();
    match err {
        AppError::Coded { code, .. } => assert_eq!(code, "fx.explicit-rate-non-positive"),
        other => panic!("应为码化错误，实际: {other}"),
    }
}

#[test]
fn normalize_errors_without_rate_for_non_default_currency() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-jpy", "acc-jpy", "cash", "JPY", 0);
    let err = normalize(
        &conn,
        &Input {
            currency_code: "JPY".into(),
            ..input(TransactionKind::Expense, 10000, "acc-jpy")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("汇率"), "实际: {err}");
}
