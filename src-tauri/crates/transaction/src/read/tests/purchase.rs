use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::TransactionListFilter;
use tauri_app_lib::ledger_transaction::TransactionPurchaseInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support;
use tauri_app_lib::test_support::ScratchDir;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};

fn item(name: &str, quantity: i64, unit_price_cents: Option<i64>) -> TransactionPurchaseInput {
    TransactionPurchaseInput {
        name: name.into(),
        quantity,
        category_id: None,
        unit_price_cents,
    }
}

fn seed_multi_item_order(conn: &rusqlite::Connection) -> String {
    test_support::seed_account(conn, "acc-a", "现金", "cash", "CNY", 0);
    create_transaction_internal(
        conn,
        TransactionInput {
            purchases: vec![
                item("猫粮", 1, Some(5990)),
                item("洗衣液", 2, None),
                item("纸巾", 3, None),
            ],
            ..crate::tests::common::make_input(
                "acc-a",
                TransactionKind::Expense,
                10000,
                "2026-01-01",
            )
        },
    )
    .unwrap()
    .id
}

#[test]
fn list_and_detail_attach_purchases_in_sort_order() {
    let conn = test_support::open();
    let id = seed_multi_item_order(&conn);

    let list = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(list.total, 1);
    let row = &list.items[0];
    assert_eq!(row.id, id);
    assert_eq!(row.purchases.len(), 3);
    assert_eq!(row.purchases[0].name, "猫粮");
    assert_eq!(row.purchases[0].quantity, 1);
    assert_eq!(row.purchases[0].unit_price_cents, Some(5990));
    assert_eq!(row.purchases[1].name, "洗衣液");
    assert_eq!(row.purchases[1].unit_price_cents, None, "可空单价原样读回");
    assert_eq!(row.purchases[2].name, "纸巾");

    let detail = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(detail.purchases.len(), 3);
    assert_eq!(detail.purchases[0].name, "猫粮");
}

#[test]
fn search_and_order_summary_attach_purchases() {
    let conn = test_support::open();
    let id = seed_multi_item_order(&conn);

    let hit = search_transactions_internal(&conn, "", 1, 50, Some(10000), Some(10000), None, None)
        .unwrap();
    assert_eq!(hit.total, 1);
    assert_eq!(hit.items[0].id, id);
    assert_eq!(hit.items[0].purchases.len(), 3, "搜索页同一读回契约");

    conn.execute(
        "UPDATE transactions SET source_order_no='JD-123' WHERE id=?1",
        rusqlite::params![id],
    )
    .unwrap();
    let summary = get_transaction_order_summary(&conn, "JD-123").unwrap();
    assert_eq!(summary.items.len(), 1);
    assert_eq!(summary.items[0].purchases.len(), 3, "订单汇总同一读回契约");
    assert_eq!(summary.items[0].purchases[1].name, "洗衣液");
}

#[test]
fn purchase_order_created_via_write_path_hits_search() {
    let conn = test_support::open();
    let id = seed_multi_item_order(&conn);

    let hit = search_transactions_internal(&conn, "猫粮", 1, 50, None, None, None, None).unwrap();
    assert_eq!(hit.total, 1, "商品名片段「猫粮」命中该订单");
    assert_eq!(hit.items[0].id, id);
    assert_eq!(
        hit.items[0].purchases.len(),
        3,
        "命中后整单读回全部购买项行，不裁剪命中行"
    );
    let hit = search_transactions_internal(&conn, "洗衣", 1, 50, None, None, None, None).unwrap();
    assert_eq!(hit.total, 1);
    assert_eq!(hit.items[0].id, id);
}

#[test]
fn legacy_rows_read_back_with_empty_purchases() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "现金", "cash", "CNY", 0);
    create_transaction_internal(
        &conn,
        crate::tests::common::make_input("acc-a", TransactionKind::Expense, 1000, "2026-01-01"),
    )
    .unwrap();

    let list = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert!(
        list.items[0].purchases.is_empty(),
        "无购买项的行 purchases 恒空"
    );
}

#[test]
fn soft_deleted_transaction_reads_back_without_purchases() {
    let conn = test_support::open();
    let id = seed_multi_item_order(&conn);
    delete_transaction_internal(&conn, &id).unwrap();

    let list = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(list.total, 0, "软删行整体不参与读回");
    assert!(get_transaction_internal(&conn, &id).is_err());
}

#[test]
fn detail_main_row_and_purchases_share_one_snapshot() {
    let dir = ScratchDir::new("tx-detail-purchase-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_multi_item_order(&conn);

    let baseline = get_transaction_internal(&conn, &id).unwrap();

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM transaction_purchases WHERE transaction_id IN",
        &[&format!(
            "UPDATE transaction_purchases SET name = '漂移' \
             WHERE transaction_id = '{id}' AND sort = 0"
        )],
    );
    let after = get_transaction_internal(&conn, &id).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中详情读闭包（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        baseline.purchases, after.purchases,
        "详情主行与购买项子行必须同快照——注入写要么整体进快照、要么整体不进"
    );
}

#[test]
fn list_page_rows_and_purchases_share_one_snapshot() {
    let dir = ScratchDir::new("tx-list-purchase-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_multi_item_order(&conn);

    let baseline = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM transaction_purchases WHERE transaction_id IN",
        &[&format!(
            "UPDATE transaction_purchases SET name = '漂移' \
             WHERE transaction_id = '{id}' AND sort = 0"
        )],
    );
    let after = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中列表读闭包（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        baseline.items[0].purchases, after.items[0].purchases,
        "列表页行与购买项子行必须同快照——注入写要么整体进快照、要么整体不进"
    );
}

#[test]
fn search_hit_set_and_rows_share_one_snapshot() {
    let dir = ScratchDir::new("tx-search-purchase-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_multi_item_order(&conn);

    let baseline =
        search_transactions_internal(&conn, "猫粮", 1, 50, None, None, None, None).unwrap();
    assert_eq!(baseline.total, 1, "夹具前置：搜索「猫粮」命中样例订单");

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "SELECT DISTINCT transaction_id FROM transaction_purchases",
        &[&format!(
            "UPDATE transaction_purchases SET name = '漂移' \
             WHERE transaction_id = '{id}' AND sort = 0"
        )],
    );
    let after = search_transactions_internal(&conn, "猫粮", 1, 50, None, None, None, None).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中搜索读闭包（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        baseline.total, after.total,
        "命中 id 集合与行集/总数必须同快照——注入写要么整体进快照、要么整体不进"
    );
    assert_eq!(
        baseline.items[0].purchases, after.items[0].purchases,
        "命中展示的购买项子行与命中判定必须同快照"
    );
}
