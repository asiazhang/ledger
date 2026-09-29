//! 购买项写入协议路径（issue #1882 / ADR-0138 决策 9/15 + 影响节）：创建 op 载荷
//! 随明细走、协议准入段拒绝投资 kind 携带、修改全量替换、软删子行失效、
//! 同步重放两端收敛（购买项随交易命令载荷走，无新实体标签）。

use ledger_sync_engine::{DomainCommand, read_ops};
use tauri_app_lib::ledger_transaction::TransactionCommand;
use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::TransactionPurchaseInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::test_support;

use crate::tests::common::make_input;

/// 购买项条目构造器。
fn item(name: &str, quantity: i64, unit_price_cents: Option<i64>) -> TransactionPurchaseInput {
    TransactionPurchaseInput {
        name: name.into(),
        quantity,
        category_id: None,
        unit_price_cents,
    }
}

/// 多商品订单入参（3 件商品，源单只给总额 → 单价留空）。
fn multi_item_input() -> TransactionInput {
    TransactionInput {
        purchases: vec![
            item("猫粮", 1, Some(5990)),
            item("洗衣液", 2, None),
            item("纸巾", 3, None),
        ],
        ..make_input("acc-a", TransactionKind::Expense, 10000, "2026-01-01")
    }
}

fn seed(conn: &rusqlite::Connection) {
    test_support::seed_account(conn, "acc-a", "现金", "cash", "CNY", 0);
}

// ---------------------------------------------------------------------------
// 创建：op 载荷随明细走（影响节：购买项随交易命令载荷走，无新实体标签）
// ---------------------------------------------------------------------------

#[test]
fn create_op_payload_carries_purchases() {
    let conn = test_support::open();
    seed(&conn);
    let created =
        tauri_app_lib::ledger_transaction::create_transaction_internal(&conn, multi_item_input())
            .unwrap();

    let ops = read_ops(&conn).unwrap();
    assert_eq!(ops.len(), 1);
    let DomainCommand::Transaction(TransactionCommand::Create { id, row, .. }) = &ops[0].command
    else {
        panic!("应为 create 命令");
    };
    assert_eq!(id, &created.id);
    assert_eq!(
        row.purchases.len(),
        3,
        "购买项随 op 搬运（Replay 形态同语义）"
    );
    assert_eq!(row.purchases[0].name, "猫粮");
    assert_eq!(row.purchases[0].unit_price_cents, Some(5990));
    assert_eq!(row.purchases[1].unit_price_cents, None, "可空单价随载荷");
}

// ---------------------------------------------------------------------------
// 协议准入段：投资 kind 携带购买项码化拒绝（guard_reference_admission）
// ---------------------------------------------------------------------------

#[test]
fn create_rejects_purchases_on_investment_kinds() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-inv", "acc-inv", "investment", "CNY", 0);
    let mut input = crate::tests::common::make_buy_input("acc-inv", "inst-guard", 1.0, 100, 0);
    input.purchases = vec![item("猫粮", 1, None)];
    let err = tauri_app_lib::ledger_transaction::create(&conn, input).unwrap_err();
    assert_eq!(err.code(), Some("transaction.purchase-item-unsupported"));
}

// ---------------------------------------------------------------------------
// 修改：全量替换（空 ⇔ 移除；换明细 ⇔ 重写）
// ---------------------------------------------------------------------------

#[test]
fn update_replaces_purchases_with_full_semantics() {
    let conn = test_support::open();
    seed(&conn);
    let created =
        tauri_app_lib::ledger_transaction::create_transaction_internal(&conn, multi_item_input())
            .unwrap();

    // 换明细：全量重写（旧 3 行 → 新 1 行）。
    tauri_app_lib::ledger_transaction::update_transaction_internal(
        &conn,
        &created.id,
        TransactionInput {
            purchases: vec![item("洗衣液", 1, None)],
            ..make_input("acc-a", TransactionKind::Expense, 10000, "2026-01-01")
        },
    )
    .unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transaction_purchases WHERE transaction_id=?1",
            rusqlite::params![created.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "修改为全量替换，无旧形态残留");
    let detail =
        tauri_app_lib::ledger_transaction::get_transaction_internal(&conn, &created.id).unwrap();
    assert_eq!(detail.purchases[0].name, "洗衣液");

    // 空 ⇔ 移除：不带 purchases 的修改整体移除明细。
    tauri_app_lib::ledger_transaction::update_transaction_internal(
        &conn,
        &created.id,
        make_input("acc-a", TransactionKind::Expense, 10000, "2026-01-01"),
    )
    .unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transaction_purchases WHERE transaction_id=?1",
            rusqlite::params![created.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "空明细 = 整体移除");
}

// ---------------------------------------------------------------------------
// 软删：子行随主行失效（无独立软删位，不新增清理机制）
// ---------------------------------------------------------------------------

#[test]
fn delete_invalidates_purchases_without_deleting_rows() {
    let conn = test_support::open();
    seed(&conn);
    let created =
        tauri_app_lib::ledger_transaction::create_transaction_internal(&conn, multi_item_input())
            .unwrap();

    tauri_app_lib::ledger_transaction::delete_transaction_internal(&conn, &created.id).unwrap();

    // 子行无独立软删位：行保留在库（主行硬删才 CASCADE），不新增清理机制；
    // 失效由主行 is_deleted 口径承载（读回为空由 read/tests/purchase.rs 断言）。
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transaction_purchases WHERE transaction_id=?1",
            rusqlite::params![created.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 3, "软删不改写子行");
}

// ---------------------------------------------------------------------------
// 同步重放：购买项随交易命令载荷走，两端收敛
// ---------------------------------------------------------------------------

#[test]
fn replay_converges_purchases_to_the_other_end() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed(&conn_a);
    seed(&conn_b);

    let created =
        tauri_app_lib::ledger_transaction::create_transaction_internal(&conn_a, multi_item_input())
            .unwrap();
    let ops = read_ops(&conn_a).unwrap();
    let DomainCommand::Transaction(cmd) = &ops[0].command else {
        panic!("应为交易命令");
    };

    // 重放端同语义落子表：读回 3 条、顺序一致、单价可空原样（购买项随交易命令
    // 载荷走，无新实体标签、无协议改动，ADR-0138 影响节）。
    tauri_app_lib::ledger_transaction::replay_command(&conn_b, cmd).unwrap();
    let replayed =
        tauri_app_lib::ledger_transaction::get_transaction_internal(&conn_b, &created.id).unwrap();
    assert_eq!(replayed.purchases.len(), 3);
    assert_eq!(replayed.purchases[0].name, "猫粮");
    assert_eq!(replayed.purchases[2].name, "纸巾");
    assert_eq!(replayed.purchases[1].unit_price_cents, None);
}
