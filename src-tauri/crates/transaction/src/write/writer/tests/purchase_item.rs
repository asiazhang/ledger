use tauri_app_lib::ledger_transaction::TransactionPurchaseInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::purchase_items::PurchaseItem;
use tauri_app_lib::ledger_transaction::write::writer::{Input, insert_row, normalize};
use tauri_app_lib::test_support;

use super::common::input;

fn item(
    name: &str,
    quantity: i64,
    category_id: Option<&str>,
    unit_price_cents: Option<i64>,
) -> TransactionPurchaseInput {
    TransactionPurchaseInput {
        name: name.into(),
        quantity,
        category_id: category_id.map(String::from),
        unit_price_cents,
    }
}

fn purchased_input(
    kind: TransactionKind,
    amount_cents: i64,
    purchases: Vec<TransactionPurchaseInput>,
) -> Input {
    Input {
        purchases: purchases.iter().map(PurchaseItem::from).collect(),
        ..input(kind, amount_cents, "acc-a")
    }
}

#[test]
fn normalize_rejects_purchases_on_transfer() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &Input {
            to_account_id: Some("acc-b".into()),
            ..purchased_input(
                TransactionKind::Transfer,
                1000,
                vec![item("猫粮", 1, None, None)],
            )
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.purchase-item-unsupported"));
    assert_eq!(err.to_string(), "交易类型 transfer 不能携带购买项");
}

#[test]
fn normalize_rejects_purchases_on_income() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &purchased_input(
            TransactionKind::Income,
            1000,
            vec![item("猫粮", 1, None, None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.purchase-item-unsupported"));
}

#[test]
fn normalize_rejects_purchases_on_refund() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-src", "来源", "cash", "CNY", 0);
    let source_id = {
        let norm = normalize(&conn, &input(TransactionKind::Expense, 1000, "acc-src")).unwrap();
        insert_row(&conn, &norm).unwrap()
    };
    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(source_id),
            ..purchased_input(
                TransactionKind::Refund,
                500,
                vec![item("猫粮", 1, None, None)],
            )
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.purchase-item-unsupported"));
}

#[test]
fn normalize_rejects_empty_name() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &purchased_input(
            TransactionKind::Expense,
            1000,
            vec![item("", 1, None, None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.purchase-name-required"));
}

#[test]
fn normalize_rejects_non_positive_quantity() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &purchased_input(
            TransactionKind::Expense,
            1000,
            vec![item("猫粮", 0, None, None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.purchase-quantity-positive"));
}

#[test]
fn normalize_rejects_negative_price_but_allows_zero() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &purchased_input(
            TransactionKind::Expense,
            1000,
            vec![item("猫粮", 1, None, Some(-1))],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("transaction.purchase-price-negative"));

    let norm = normalize(
        &conn,
        &purchased_input(
            TransactionKind::Expense,
            1000,
            vec![item("赠品", 1, None, Some(0))],
        ),
    )
    .unwrap();
    assert_eq!(norm.purchases[0].unit_price_cents, Some(0));
}

#[test]
fn normalize_rejects_missing_purchase_category() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(
        &conn,
        &purchased_input(
            TransactionKind::Expense,
            1000,
            vec![item("猫粮", 1, Some("no-such-cat"), None)],
        ),
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("purchase.category-not-found"));
}

#[test]
fn insert_row_persists_purchase_rows_in_array_order() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &purchased_input(
            TransactionKind::Expense,
            10000,
            vec![
                item("猫粮", 1, None, Some(5990)),
                item("洗衣液", 2, None, None),
                item("纸巾", 3, None, Some(0)),
            ],
        ),
    )
    .unwrap();
    let id = insert_row(&conn, &norm).unwrap();

    let rows: Vec<(i64, String, i64, Option<i64>)> = conn
        .prepare(
            "SELECT sort, name, quantity, unit_price_cents \
             FROM transaction_purchases WHERE transaction_id=?1 ORDER BY sort",
        )
        .unwrap()
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (0, "猫粮".into(), 1, Some(5990)),
            (1, "洗衣液".into(), 2, None),
            (2, "纸巾".into(), 3, Some(0)),
        ]
    );
}

#[test]
fn purchase_prices_do_not_touch_balances() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &purchased_input(
            TransactionKind::Expense,
            1000,
            vec![item("猫粮", 1, None, Some(999_999))],
        ),
    )
    .unwrap();
    let id = insert_row(&conn, &norm).unwrap();
    let cached: i64 = conn
        .query_row(
            "SELECT balance_cents FROM account_balance_cache WHERE account_id='acc-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cached, -1000, "余额 = 交易金额，与购买项标价无关");
    let _ = id;
}

#[test]
fn no_purchases_writes_no_rows() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let norm = normalize(&conn, &input(TransactionKind::Expense, 1000, "acc-a")).unwrap();
    let id = insert_row(&conn, &norm).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transaction_purchases WHERE transaction_id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

fn insert_purchased_expense(conn: &rusqlite::Connection) -> String {
    let norm = normalize(
        conn,
        &purchased_input(
            TransactionKind::Expense,
            1000,
            vec![item("猫粮", 1, None, None)],
        ),
    )
    .unwrap();
    insert_row(conn, &norm).unwrap()
}

#[test]
fn normalize_keeps_historical_reference_for_unchanged_purchases() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let cat = crate::tests::common::seed_category(&conn, "宠物", "expense");
    let id = {
        let norm = normalize(
            &conn,
            &purchased_input(
                TransactionKind::Expense,
                1000,
                vec![item("猫粮", 1, Some(&cat), None)],
            ),
        )
        .unwrap();
        insert_row(&conn, &norm).unwrap()
    };
    conn.execute(
        "UPDATE categories SET is_deleted=1 WHERE id=?1",
        rusqlite::params![cat],
    )
    .unwrap();
    let existing =
        tauri_app_lib::ledger_transaction::write::purchase_items::read_rows(&conn, &id).unwrap();
    let norm = normalize(
        &conn,
        &Input {
            existing_purchases: existing,
            ..purchased_input(
                TransactionKind::Expense,
                1000,
                vec![item("猫粮", 1, Some(&cat), None)],
            )
        },
    )
    .unwrap();
    assert_eq!(norm.purchases.len(), 1);
}

#[test]
fn normalize_rejects_changed_purchases_with_dead_category() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let cat = crate::tests::common::seed_category(&conn, "日用品", "expense");
    let id = insert_purchased_expense(&conn);
    conn.execute(
        "UPDATE categories SET is_deleted=1 WHERE id=?1",
        rusqlite::params![cat],
    )
    .unwrap();
    let existing =
        tauri_app_lib::ledger_transaction::write::purchase_items::read_rows(&conn, &id).unwrap();
    let err = normalize(
        &conn,
        &Input {
            existing_purchases: existing,
            ..purchased_input(
                TransactionKind::Expense,
                1000,
                vec![item("洗衣液", 1, Some(&cat), None)],
            )
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("purchase.category-not-found"));
}
