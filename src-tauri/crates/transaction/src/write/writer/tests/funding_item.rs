use ledger_infra::error::{AppError, ErrClass};
use tauri_app_lib::ledger_transaction::TransactionFundingInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::funding_items::FundingItem;
use tauri_app_lib::ledger_transaction::write::writer::{Input, insert_row, normalize};
use tauri_app_lib::test_support;

use super::common::input;

fn item(account_id: &str, amount_cents: i64, label: Option<&str>) -> TransactionFundingInput {
    TransactionFundingInput {
        account_id: account_id.into(),
        amount_cents,
        label: label.map(String::from),
    }
}

fn funded_input(
    kind: TransactionKind,
    amount_cents: i64,
    funding: Vec<TransactionFundingInput>,
) -> Input {
    Input {
        kind,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: None,
        funding: funding.iter().map(FundingItem::from).collect(),
        ..input(kind, amount_cents, "")
    }
}

#[test]
fn normalize_rejects_funding_with_account_id() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &Input {
            account_id: Some("acc-a".into()),
            ..funded_input(
                TransactionKind::Expense,
                1000,
                vec![item("acc-b", 1000, None)],
            )
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-account-conflict"));
}

#[test]
fn normalize_requires_account_when_no_funding() {
    let conn = test_support::open();
    let err = normalize(
        &conn,
        &Input {
            account_id: None,
            ..input(TransactionKind::Expense, 1000, "")
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.account-required"));
}

#[test]
fn normalize_rejects_funding_on_transfer() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &Input {
            to_account_id: Some("acc-a".into()),
            ..funded_input(
                TransactionKind::Transfer,
                1000,
                vec![item("acc-a", 1000, None)],
            )
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-item-unsupported"));
}

#[test]
fn create_rejects_funding_on_investment_kinds() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-inv", "acc-inv", "investment", "CNY", 0);
    let mut input = crate::tests::common::make_buy_input("acc-inv", "inst-guard", 1.0, 100, 0);
    input.funding = vec![item("acc-inv", 1, None)];
    let err = tauri_app_lib::ledger_transaction::create(&conn, input).unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-item-unsupported"));
}

#[test]
fn normalize_rejects_sum_mismatch() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-a", 2853, None), item("acc-b", 25307, None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-sum-mismatch"));
    assert_eq!(
        err.to_string(),
        "出资分解合计（28160）与交易金额（1000）不一致"
    );
}

#[test]
fn normalize_rejects_non_positive_item_amount() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-a", 1000, None), item("acc-a", 0, None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-amount-positive"));
}

#[test]
fn normalize_rejects_label_over_50_chars() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let label_51 = "标".repeat(51);
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-a", 1000, Some(&label_51))],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.funding-label-too-long"));

    let label_50 = "标".repeat(50);
    let norm = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-a", 1000, Some(&label_50))],
        ),
    )
    .unwrap();
    assert_eq!(norm.funding.len(), 1);
}

#[test]
fn normalize_rejects_missing_funding_account() {
    let conn = test_support::open();
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("no-such-acc", 1000, None)],
        ),
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
fn normalize_rejects_funding_currency_mismatch() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    let err = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-usd", 1000, None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("funding.currency-mismatch"));
}

#[test]
fn normalize_allows_credit_and_debt_accounts_as_funding() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-credit", "信用卡", "credit", "CNY", 0);
    test_support::seed_account(&conn, "acc-debt", "白条", "debt", "CNY", 0);
    let norm = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![
                item("acc-credit", 300, Some("定金")),
                item("acc-debt", 700, Some("尾款")),
            ],
        ),
    )
    .unwrap();
    assert_eq!(norm.account_id, None, "分解行主表账户列落 NULL");
    assert_eq!(norm.funding.len(), 2);
    assert_eq!(norm.funding[0].label.as_deref(), Some("定金"));
    assert_eq!(norm.funding[1].amount_cents, 700);
}

#[test]
fn normalize_allows_same_account_multiple_items() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &funded_input(
            TransactionKind::Income,
            1000,
            vec![item("acc-a", 400, Some("定金")), item("acc-a", 600, None)],
        ),
    )
    .unwrap();
    assert_eq!(norm.funding.len(), 2);
}

#[test]
fn insert_row_persists_funding_rows_and_refreshes_balances() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-wallet", "小金库", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-balance", "余额", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &funded_input(
            TransactionKind::Expense,
            28160,
            vec![
                item("acc-wallet", 2853, None),
                item("acc-balance", 25307, None),
            ],
        ),
    )
    .unwrap();
    let id = insert_row(&conn, &norm).unwrap();

    let sort_account: Vec<(i64, String, i64)> = conn
        .prepare("SELECT sort, account_id, amount_cents FROM transaction_fundings WHERE transaction_id=?1 ORDER BY sort")
        .unwrap()
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        sort_account,
        vec![
            (0, "acc-wallet".into(), 2853),
            (1, "acc-balance".into(), 25307)
        ]
    );

    let cached = |id: &str| -> i64 {
        conn.query_row(
            "SELECT balance_cents FROM account_balance_cache WHERE account_id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(cached("acc-wallet"), -2853);
    assert_eq!(cached("acc-balance"), -25307);
}

fn insert_funded_expense(conn: &rusqlite::Connection) -> String {
    let norm = normalize(
        conn,
        &funded_input(
            TransactionKind::Expense,
            1000,
            vec![item("acc-w1", 100, None), item("acc-b1", 900, None)],
        ),
    )
    .unwrap();
    insert_row(conn, &norm).unwrap()
}

#[test]
fn normalize_refund_inherits_null_account_from_funded_expense() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-w1", "小金库", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b1", "余额", "cash", "CNY", 0);
    let source_id = insert_funded_expense(&conn);

    let norm = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 500, "")
        },
    )
    .unwrap();
    assert_eq!(norm.account_id, None);
    assert!(norm.funding.is_empty());
}

#[test]
fn normalize_refund_explicit_override_persists_breakdown() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-w1", "小金库", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b1", "余额", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-x", "精准回退", "cash", "CNY", 0);
    let source_id = insert_funded_expense(&conn);

    let norm = normalize(
        &conn,
        &Input {
            account_id: None,
            refund_of_transaction_id: Some(source_id),
            funding: vec![FundingItem::from(&item("acc-x", 500, None))],
            ..input(TransactionKind::Refund, 500, "")
        },
    )
    .unwrap();
    assert_eq!(norm.account_id, None);
    assert_eq!(norm.funding.len(), 1);
    assert_eq!(norm.funding[0].account_id, "acc-x");
}
