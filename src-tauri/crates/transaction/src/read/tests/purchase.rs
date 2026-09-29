//! 购买项读回与读闭包（issue #1882 / ADR-0138 决策 9/10/15）：列表/详情/搜索/
//! 订单汇总 attach、软删子行失效、读快照探针（删除接线即红）。
//!
//! 全部经根包实例驱动（行为路径测试纪律，见 crate 根 lib.rs 头注）。

use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::TransactionListFilter;
use tauri_app_lib::ledger_transaction::TransactionPurchaseInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support;
use tauri_app_lib::test_support::ScratchDir;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};

/// 购买项条目构造器。
fn item(name: &str, quantity: i64, unit_price_cents: Option<i64>) -> TransactionPurchaseInput {
    TransactionPurchaseInput {
        name: name.into(),
        quantity,
        category_id: None,
        unit_price_cents,
    }
}

/// 多商品订单样例（3 件商品，源单只给总额 → 单价留空）。
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

// ---------------------------------------------------------------------------
// 列表 / 详情 / 搜索 / 订单汇总同一读回契约（读回非空明细 ⇔ 存在购买项子行）
// ---------------------------------------------------------------------------

#[test]
fn list_and_detail_attach_purchases_in_sort_order() {
    let conn = test_support::open();
    let id = seed_multi_item_order(&conn);

    // 列表：购买项按顺序位（对账单顺序）稳定返回，含可空单价。
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

    // 详情：同一 attach 契约。
    let detail = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(detail.purchases.len(), 3);
    assert_eq!(detail.purchases[0].name, "猫粮");
}

#[test]
fn search_and_order_summary_attach_purchases() {
    let conn = test_support::open();
    let id = seed_multi_item_order(&conn);

    // 搜索页（仅筛选路径同样回表取展示列）。
    let hit = search_transactions_internal(&conn, "", 1, 50, Some(10000), Some(10000), None, None)
        .unwrap();
    assert_eq!(hit.total, 1);
    assert_eq!(hit.items[0].id, id);
    assert_eq!(hit.items[0].purchases.len(), 3, "搜索页同一读回契约");

    // 订单汇总（同单行集）：明细与列表同形。
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

/// 端到端验收（issue #1885）：经写入通道提交一笔含「渴望 猫粮」的订单 → 搜索
/// 「猫粮」命中该交易（商品名片段命中 + 整单读回，全部经公开写入口驱动）。
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
    // 中段片段同样命中（同单另一商品）。
    let hit = search_transactions_internal(&conn, "洗衣", 1, 50, None, None, None, None).unwrap();
    assert_eq!(hit.total, 1);
    assert_eq!(hit.items[0].id, id);
}

/// 无购买项的存量交易行为零变化：`purchases` 恒空数组。
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

/// 交易软删后购买项随之失效：读回不再出现（子行无独立软删位）。
#[test]
fn soft_deleted_transaction_reads_back_without_purchases() {
    let conn = test_support::open();
    let id = seed_multi_item_order(&conn);
    delete_transaction_internal(&conn, &id).unwrap();

    let list = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(list.total, 0, "软删行整体不参与读回");
    assert!(get_transaction_internal(&conn, &id).is_err());
}

// ---------------------------------------------------------------------------
// 读快照探针（读闭包纪律：删除接线即红，issue #1882 / ADR-0138 影响节）
// ---------------------------------------------------------------------------

/// 详情读回是多语句读闭包（主行 SELECT ↔ 购买项子行 SELECT）：写提交落在两语句
/// 之间会读到「主行旧、子行新」的混搭行。探针在子行查询开始前于另一连接改写
/// 购买项名称——
/// - 读闭包无快照保护（红）：主行读旧子行读新，purchases 与基线漂移；
/// - 读闭包收进读事务（绿）：注入写被挡，详情与基线同快照。
#[test]
fn detail_main_row_and_purchases_share_one_snapshot() {
    let dir = ScratchDir::new("tx-detail-purchase-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_multi_item_order(&conn);

    let baseline = get_transaction_internal(&conn, &id).unwrap();

    // 探针：marker = 子行 SELECT（详情闭包中 fundings 之后的语句）——注入落在
    // 主行读后、子行读前，另一连接把首条购买项名称改写。
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

/// 列表读闭包（页行 SELECT ↔ 购买项子行 SELECT）同快照——本票验收判据
/// 「列表页行集 ↔ 购买项子行的读闭包纪律」的探针载体（删除列表闭包内
/// attach_purchases 接线本测试变红）。
#[test]
fn list_page_rows_and_purchases_share_one_snapshot() {
    let dir = ScratchDir::new("tx-list-purchase-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_multi_item_order(&conn);

    let baseline = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();

    // 探针：marker = 子行 SELECT——注入落在页行读后、子行读前，另一连接把首条
    // 购买项名称改写。
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

/// 搜索读闭包（购买项命中 id 集合预查询 ↔ 命中行集/总数/页回表/子行 attach）
/// 同快照——本票验收判据「命中 id 集合与行集同口径」的探针载体（issue #1885；
/// 删除搜索闭包内 ensure_transaction 接线本测试变红：注入写落在预查询后，
/// 命中集合按旧名命中、页行集按新名漂移）。
#[test]
fn search_hit_set_and_rows_share_one_snapshot() {
    let dir = ScratchDir::new("tx-search-purchase-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_multi_item_order(&conn);

    let baseline =
        search_transactions_internal(&conn, "猫粮", 1, 50, None, None, None, None).unwrap();
    assert_eq!(baseline.total, 1, "夹具前置：搜索「猫粮」命中样例订单");

    // 探针：marker = 命中 id 预查询——注入落在字典装载后、预查询起点，另一连接
    // 把首条购买项名称改写（猫粮 → 漂移）。
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
