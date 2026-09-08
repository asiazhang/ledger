//! op 产出（issue #855 / ADR-0091）：交易创建/修改/删除经行为编排入口成功后，
//! 对应 op（含源端折算字段）已追加且不重复；写失败不残留 op。
//!
//! 断言权威在同步引擎公开接口（`sync_engine::read_ops`）——本文件只经由它
//! 读取 op，不直查 `sync_ops` 表。

use rusqlite::Connection;

use super::super::*;
use super::common::{make_buy_input, make_input};
use crate::db;
use crate::sync_engine::{DomainCommand, SyncOp, device_id, read_ops};
use crate::test_support;
use crate::transaction::TransactionCommand;
use crate::transaction::amount::TransactionKind;

/// 读全部 op 的便捷形态（断言权威：同步引擎公开接口）。
fn ops(conn: &Connection) -> Vec<SyncOp> {
    read_ops(conn).unwrap()
}

#[test]
fn create_appends_one_op_with_source_folding() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-op", "现金", "cash", "CNY", 0);

    let created = create_transaction_internal(
        &conn,
        make_input("acc-op", TransactionKind::Expense, 10000, "2026-01-10"),
    )
    .unwrap();

    let ops = ops(&conn);
    assert_eq!(ops.len(), 1, "一次创建恰好追加一条 op");
    let op = &ops[0];
    // 信封字段：来源设备、端内时钟从 1 起单调、产生时 schema 版本。
    assert_eq!(op.device_id, device_id(&conn).unwrap());
    assert_eq!(op.clock, 1);
    assert_eq!(op.schema_version, db::schema_version(&conn).unwrap());
    // 载荷：create 命令携带实体 id 与归一化行（含源端折算 amount_native_cents）。
    let DomainCommand::Transaction(TransactionCommand::Create {
        id,
        row,
        investment,
    }) = &op.command
    else {
        panic!("应为 create 命令，实际: {:?}", op.command);
    };
    assert_eq!(id, &created.id);
    assert_eq!(row.kind, TransactionKind::Expense);
    assert_eq!(row.amount_cents, 10000);
    assert_eq!(row.amount_native_cents, 10000, "源端折算结果随 op 携带");
    assert_eq!(row.account_id, "acc-op");
    assert!(investment.is_none(), "普通 kind 不携带投资字段");
}

#[test]
fn update_and_delete_append_exactly_one_op_each() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-op", "现金", "cash", "CNY", 0);

    let id = create_transaction_internal(
        &conn,
        make_input("acc-op", TransactionKind::Expense, 10000, "2026-01-10"),
    )
    .unwrap()
    .id;
    update_transaction_internal(
        &conn,
        &id,
        make_input("acc-op", TransactionKind::Expense, 20000, "2026-01-10"),
    )
    .unwrap();
    delete_transaction_internal(&conn, &id).unwrap();

    let ops = ops(&conn);
    assert_eq!(ops.len(), 3, "创建/修改/删除各恰好一条 op");
    assert_eq!(ops[0].clock, 1);
    assert_eq!(ops[1].clock, 2, "端内时钟严格递增");
    assert_eq!(ops[2].clock, 3);
    let DomainCommand::Transaction(TransactionCommand::Update { id: uid, row, .. }) =
        &ops[1].command
    else {
        panic!("应为 update 命令");
    };
    assert_eq!(uid, &id);
    assert_eq!(row.amount_native_cents, 20000, "修改 op 携带折算后的新行");
    let DomainCommand::Transaction(TransactionCommand::Delete { id: did }) = &ops[2].command else {
        panic!("应为 delete 命令");
    };
    assert_eq!(did, &id);
}

#[test]
fn failed_write_appends_no_op() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-op", "现金", "cash", "CNY", 0);

    // 金额必须 > 0：校验失败不落库也不产 op。
    assert!(
        create_transaction_internal(
            &conn,
            make_input("acc-op", TransactionKind::Expense, 0, "2026-01-10"),
        )
        .is_err()
    );
    assert_eq!(ops(&conn).len(), 0, "写失败不残留 op");

    // 删除不存在的 id：协议报错，不产 op。
    assert!(delete_transaction_internal(&conn, "no-such-id").is_err());
    assert_eq!(ops(&conn).len(), 0);

    // 修改不存在的 id：同上。
    assert!(
        update_transaction_internal(
            &conn,
            "no-such-id",
            make_input("acc-op", TransactionKind::Expense, 100, "2026-01-10"),
        )
        .is_err()
    );
    assert_eq!(ops(&conn).len(), 0);
}

#[test]
fn refund_op_carries_inherited_normalized_row() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-op", "现金", "cash", "CNY", 0);

    let expense_id = create_transaction_internal(
        &conn,
        make_input("acc-op", TransactionKind::Expense, 8000, "2026-01-10"),
    )
    .unwrap()
    .id;
    let mut refund_input = make_input("other-acc", TransactionKind::Refund, 3000, "2026-01-11");
    refund_input.refund_of_transaction_id = Some(expense_id.clone());
    create_transaction_internal(&conn, refund_input).unwrap();

    let ops = ops(&conn);
    assert_eq!(ops.len(), 2);
    let DomainCommand::Transaction(TransactionCommand::Create { row, .. }) = &ops[1].command else {
        panic!("应为 create 命令");
    };
    // 归一化结果（退款继承原支出账户）随 op 携带：重放端不再重推继承。
    assert_eq!(row.kind, TransactionKind::Refund);
    assert_eq!(row.account_id, "acc-op");
    assert_eq!(
        row.refund_of_transaction_id.as_deref(),
        Some(expense_id.as_str())
    );
}

#[test]
fn buy_op_carries_investment_fields_for_audit() {
    let conn = test_support::open();
    crate::test_support::seed_investment_setup(&conn, "acc-inv", "inst-1");

    let id =
        create_transaction_internal(&conn, make_buy_input("acc-inv", "inst-1", 10.0, 15000, 100))
            .unwrap()
            .id;

    let ops = ops(&conn);
    assert_eq!(ops.len(), 1);
    let DomainCommand::Transaction(TransactionCommand::Create {
        id: cid,
        row,
        investment,
    }) = &ops[0].command
    else {
        panic!("应为 create 命令");
    };
    assert_eq!(cid, &id);
    assert_eq!(row.kind, TransactionKind::Buy);
    // 折算结果（源端）与投资语义字段都随 op 留痕（投资命令重放执行待 #861）。
    // 行金额 = 数量 × 单价（万分之一元 → 分）+ 手续费 = 10 × 1.5 元 + 1 元。
    assert_eq!(row.amount_native_cents, 1600);
    let inv = investment.as_ref().expect("buy 应携带投资字段");
    assert_eq!(inv.instrument_id, "inst-1");
    assert!((inv.quantity - 10.0).abs() < f64::EPSILON);
    assert_eq!(inv.price_cents, 15000);
    assert_eq!(inv.fee_cents, 100);
}
