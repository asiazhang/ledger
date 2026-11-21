use crate::tests::common::{make_input, seed_category};
use ledger_infra::error::AppError;
use rusqlite::{Connection, params};
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support;

fn assert_coded_rejection(err: AppError, code: &str, message_part: &str) {
    match err {
        AppError::Coded {
            code: actual,
            message,
            ..
        } => {
            assert_eq!(actual, code);
            assert!(
                message.contains(message_part),
                "应含「{message_part}」，实际: {message}"
            );
        }
        other => panic!("应为码化错误，实际 {other:?}"),
    }
}

fn category_id_of(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT category_id FROM transactions WHERE id=?1",
        params![id],
        |r| r.get(0),
    )
    .unwrap()
}

fn active_txn_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn create_expense_income_carry_category_transfer_buy_sell_rejected() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-c", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-c-to", "银行", "bank", "CNY", 0);
    test_support::seed_account(&conn, "acc-c-inv", "证券", "investment", "CNY", 0);
    let cat_food = seed_category(&conn, "餐饮", "expense");
    let cat_salary = seed_category(&conn, "工资", "income");

    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some(cat_food.clone()),
            ..make_input("acc-c", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;
    let income_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some(cat_salary.clone()),
            ..make_input("acc-c", TransactionKind::Income, 500, "2026-01-02")
        },
    )
    .unwrap()
    .id;
    assert_eq!(
        category_id_of(&conn, &expense_id).as_deref(),
        Some(cat_food.as_str())
    );
    assert_eq!(
        category_id_of(&conn, &income_id).as_deref(),
        Some(cat_salary.as_str())
    );

    let err = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            category_id: Some(cat_food.clone()),
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 3000, "2026-01-03")
        },
    )
    .unwrap_err();
    assert_coded_rejection(err, "transaction.category-unsupported", "不能携带分类");

    for kind in [TransactionKind::Buy, TransactionKind::Sell] {
        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                policy_id: None,
                merchant_id: None,
                kind,
                category_id: Some(cat_food.clone()),
                instrument_id: Some("inst-x".into()),
                quantity: Some(10.0),
                price_cents: Some(1000),
                fee_cents: Some(0),
                ..make_input("acc-c-inv", kind, 10000, "2026-01-04")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("不能携带分类"),
            "{kind} 应报「不能携带分类」，实际: {err}"
        );
    }

    let plain_transfer = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 3000, "2026-01-05")
        },
    )
    .unwrap()
    .id;
    assert_eq!(category_id_of(&conn, &plain_transfer), None);

    assert_eq!(active_txn_count(&conn), 3, "拒绝的交易不应落库");
}

#[test]
fn create_refund_ignores_caller_category_and_inherits_original() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-c", "现金", "cash", "CNY", 0);
    let cat_food = seed_category(&conn, "餐饮", "expense");
    let cat_toy = seed_category(&conn, "玩具", "expense");

    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some(cat_food.clone()),
            ..make_input("acc-c", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Refund,
            category_id: Some(cat_toy.clone()),
            refund_of_transaction_id: Some(expense_id),
            ..make_input("acc-c", TransactionKind::Refund, 100, "2026-01-02")
        },
    )
    .unwrap()
    .id;
    assert_eq!(
        category_id_of(&conn, &refund_id).as_deref(),
        Some(cat_food.as_str()),
        "refund 应继承原支出分类、忽略调用方填值"
    );
}

#[test]
fn create_dividend_split_with_category_reports_category_rejection_first() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-c", "现金", "cash", "CNY", 0);
    let cat_food = seed_category(&conn, "餐饮", "expense");

    for kind in [TransactionKind::Dividend, TransactionKind::Split] {
        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                policy_id: None,
                merchant_id: None,
                kind,
                category_id: Some(cat_food.clone()),
                ..make_input("acc-c", kind, 60, "2026-01-01")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("不能携带分类"),
            "{kind} 携带分类应报「不能携带分类」，实际: {err}"
        );

        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                policy_id: None,
                merchant_id: None,
                kind,
                ..make_input("acc-c", kind, 60, "2026-01-01")
            },
        )
        .unwrap_err();
        if kind == TransactionKind::Dividend {
            assert!(
                err.is_code("trade.dividend-instrument-required"),
                "dividend 不携带分类应进投资域守卫（缺标的），实际: {err}"
            );
        } else {
            assert!(
                err.is_code("trade.split-instrument-required"),
                "split 不携带分类应进投资域守卫（缺标的），实际: {err}"
            );
        }
    }

    assert_eq!(active_txn_count(&conn), 0, "全部被拒，无任何落库");
}

#[test]
fn update_to_transfer_with_category_rejected_and_rolls_back() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-c", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-c-to", "银行", "bank", "CNY", 0);
    let cat_food = seed_category(&conn, "餐饮", "expense");

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some(cat_food.clone()),
            ..make_input("acc-c", TransactionKind::Expense, 500, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    let err = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            category_id: Some(cat_food.clone()),
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 3000, "2026-01-02")
        },
    )
    .unwrap_err();
    assert_coded_rejection(err, "transaction.category-unsupported", "不能携带分类");
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Expense);
    assert_eq!(t.category_id.as_deref(), Some(cat_food.as_str()));
}

#[test]
fn batch_transfer_with_category_fails_row_without_silent_strip() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-c", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-c-to", "银行", "bank", "CNY", 0);
    let cat_food = seed_category(&conn, "餐饮", "expense");

    let inputs = vec![
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            category_id: Some(cat_food.clone()),
            ..make_input("acc-c", TransactionKind::Expense, 1000, "2026-01-01")
        },
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            category_id: Some(cat_food.clone()),
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 3000, "2026-01-02")
        },
        TransactionInput {
            policy_id: None,
            merchant_id: None,
            kind: TransactionKind::Transfer,
            to_account_id: Some("acc-c-to".into()),
            ..make_input("acc-c", TransactionKind::Transfer, 2000, "2026-01-03")
        },
    ];
    let outcome = TransactionBatch::run(&conn, inputs, false).unwrap();
    assert_eq!(outcome.results.len(), 3);
    assert!(outcome.results[0].success && outcome.results[0].id.is_some());
    assert!(
        !outcome.results[1].success && outcome.results[1].id.is_none(),
        "携带分类的转账应单行失败"
    );
    assert!(
        outcome.results[1]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("不能携带分类"),
        "失败行应携带分类拒绝错误，实际: {:?}",
        outcome.results[1].error
    );
    assert!(outcome.results[2].success && outcome.results[2].id.is_some());

    assert_eq!(active_txn_count(&conn), 2);
    let categorized_transfer: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE kind='transfer' AND category_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(categorized_transfer, 0, "不应存在带分类的转账行");
}
