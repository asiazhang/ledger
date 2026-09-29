//! 购买项契约与子行落库（issue #1882 / ADR-0138 决策 9/10）：normalize 的
//! kind 准入与逐条约束、子行随主行落库、单价可空、修改路径保持历史引用。
//!
//! 断言模块外部行为：仅 expense 可带、子行按顺序位落库、价格不进余额口径。
//! 全部基于内存库（test_support::open 已接线余额刷新钩子）。

use tauri_app_lib::ledger_transaction::TransactionPurchaseInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::purchase_items::PurchaseItem;
use tauri_app_lib::ledger_transaction::write::writer::{Input, insert_row, normalize};
use tauri_app_lib::test_support;

use super::common::input;

/// 购买项条目构造器（测试速记）。
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

/// 带购买项的入参构造器。
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

// ---------------------------------------------------------------------------
// kind 准入（ADR-0138 决策 9：仅 expense 可带）
// ---------------------------------------------------------------------------

/// transfer 携带购买项 → 码化拒绝（两端已表达，不做明细）。
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

/// income 携带购买项 → 码化拒绝（购买项只答「这一单买了什么」，收入无此语义）。
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

/// refund 携带购买项 → 码化拒绝（退货 / 部分退款的呈现另行决策，不另挂清单）。
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

// ---------------------------------------------------------------------------
// 逐条约束（ADR-0138 决策 9/10）
// ---------------------------------------------------------------------------

/// 名称缺失（空串）→ 码化错误。
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

/// 件数非正 → 码化错误。
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

/// 单价为负 → 码化错误；0 是合法标价（赠品）。
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

/// 购买项分类不存在/已软删 → 码化 NotFound（AI 可读回自纠）。
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

// ---------------------------------------------------------------------------
// 子行落库（接线负向载体：删掉 insert_row 内 insert_rows 调用本测试变红）
// ---------------------------------------------------------------------------

/// insert_row 落子行（顺序位随数组序、可空单价原样落库）：
/// 3 件商品的多商品订单端到端——写入通道提交 → 子表 3 行。
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

/// 价格不进余额口径（ADR-0138 决策 10）：带购买项的 expense 余额只看交易金额。
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

/// 无购买项的存量行为零变化：normalize/insert 全链零子表写入。
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

// ---------------------------------------------------------------------------
// 修改路径：全量替换与保持历史引用（与出资项同款语义）
// ---------------------------------------------------------------------------

/// 落一笔带购买项的 expense。
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

/// 修改路径提交与当前一致的明细（历史分类已软删）→ 不因分类准入被拒。
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
    // 分类软删后：提交同一明细（保持历史引用）照常通过。
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

/// 修改路径换用软删分类（明细有变化）→ 码化 NotFound。
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
