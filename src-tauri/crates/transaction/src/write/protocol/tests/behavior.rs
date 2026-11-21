use crate::tests::common::{make_buy_input, make_input, seed_category};
use ledger_infra::error::{AppError, ErrClass};
use rusqlite::Connection;
use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support;

use ledger_infra::db::now_iso;
use ledger_sync_protocol::device::device_id;
use rusqlite::params;
use tauri_app_lib::ledger_transaction::amount::{FxRateSource, TransactionKind};
use tauri_app_lib::ledger_transaction::write::writer::{NormalizedRow, insert_row};

#[test]
fn create_income_and_expense_transactions() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-crud", "现金", "cash", "CNY", 0);

    let id1 = create_transaction_internal(
        &conn,
        make_input("acc-crud", TransactionKind::Income, 5000, "2026-02-01"),
    )
    .unwrap()
    .id;
    let id2 = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            amount_cents: 1500,
            note: Some("午餐".into()),
            category_id: None,
            ..make_input("acc-crud", TransactionKind::Expense, 100, "2026-02-02")
        },
    )
    .unwrap()
    .id;
    assert_ne!(id1, id2);
    let row1: (TransactionKind, String, i64, Option<String>) = conn
        .query_row(
            "SELECT kind, account_id, amount_cents, note FROM transactions WHERE id=?1",
            params![id1],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(row1.0, TransactionKind::Income);
    assert_eq!(row1.2, 5000);
}

#[test]
fn create_transfer_with_to_account() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-from", "A账户", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-to", "B账户", "cash", "CNY", 0);

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            funding: Vec::new(),
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Transfer,
            amount_cents: 3000,
            currency_code: "CNY".into(),
            account_id: Some("acc-from".into()),
            to_account_id: Some("acc-to".into()),
            date: "2026-03-01".into(),
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            funding_account_id: None,
            source_order_no: None,
            note: None,
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            idempotency_key: None,
            origin: None,
            fx_rate: None,
            purchases: Vec::new(),
        },
    )
    .unwrap()
    .id;
    let (kind, from, to): (TransactionKind, String, Option<String>) = conn
        .query_row(
            "SELECT kind, account_id, to_account_id FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(kind, TransactionKind::Transfer);
    assert_eq!(from, "acc-from");
    assert_eq!(to.as_deref(), Some("acc-to"));
}

#[test]
fn delete_transaction_soft_deletes() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-del", "现金", "cash", "CNY", 0);

    let id = create_transaction_internal(
        &conn,
        make_input("acc-del", TransactionKind::Income, 1000, "2026-01-01"),
    )
    .unwrap()
    .id;
    let count_before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_before, 1);

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        params![id, now_iso(), device_id(&conn).unwrap()],
    ).unwrap();

    let count_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_after, 0);
}

#[test]
fn delete_transaction_internal_returns_not_found_for_missing_id() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-missing", "现金", "cash", "CNY", 0);

    let err = delete_transaction_internal(&conn, "不存在的id").unwrap_err();
    match err {
        AppError::Coded { message, .. } => assert!(message.contains("交易不存在")),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn delete_transaction_internal_returns_not_found_for_already_deleted() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-gone", "现金", "cash", "CNY", 0);
    let id = create_transaction_internal(
        &conn,
        make_input("acc-gone", TransactionKind::Income, 1000, "2026-01-01"),
    )
    .unwrap()
    .id;
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![id],
    )
    .unwrap();

    let err = delete_transaction_internal(&conn, &id).unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
}

#[test]
fn create_transaction_internal_dispatches_dividend_to_investment_guard() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-div", "现金", "cash", "CNY", 0);

    let err = create_transaction_internal(
        &conn,
        make_input("acc-div", TransactionKind::Dividend, 60, "2026-05-04"),
    )
    .unwrap_err();
    assert!(
        err.is_code("trade.dividend-instrument-required"),
        "dividend 缺标的应进投资域守卫，实际: {err:?}"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "拒绝的交易不应落库");
}

#[test]
fn update_transaction_rejects_dividend_and_split_kind_change() {
    for (kind, expected_code) in [
        (
            TransactionKind::Dividend,
            "trade.dividend-kind-change-forbidden",
        ),
        (TransactionKind::Split, "trade.split-kind-change-forbidden"),
    ] {
        let conn = test_support::open();
        test_support::seed_account(&conn, "acc-unsup-upd", "现金", "cash", "CNY", 0);
        let id = create_transaction_internal(
            &conn,
            make_input("acc-unsup-upd", TransactionKind::Expense, 500, "2026-01-01"),
        )
        .unwrap()
        .id;

        let err = update_transaction_internal(
            &conn,
            &id,
            make_input("acc-unsup-upd", kind, 60, "2026-05-04"),
        )
        .unwrap_err();
        assert_eq!(err.code().unwrap(), expected_code, "改入 {kind}: {err:?}");

        let t = get_transaction_internal(&conn, &id).unwrap();
        assert_eq!(t.kind, TransactionKind::Expense);
        assert_eq!(t.amount_cents, 500);
    }
}

#[test]
fn update_transaction_cross_kind_rebuilds_side_effects_atomically() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cash-x", "现金", "cash", "CNY", 0);
    test_support::seed_investment_setup(&conn, "acc-x", "inst-x");

    let id = create_transaction_internal(
        &conn,
        make_input("acc-cash-x", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap()
    .id;
    update_transaction_internal(
        &conn,
        &id,
        make_buy_input("acc-x", "inst-x", 3.0, 100000, 0),
    )
    .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Buy);
    let lots: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lots, 1, "expense→buy 应建仓一个 lot");

    update_transaction_internal(
        &conn,
        &id,
        make_input("acc-cash-x", TransactionKind::Expense, 700, "2026-02-01"),
    )
    .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Expense);
    let (lots_after, stx_after): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1), \
                    (SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1)",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(lots_after, 0, "buy→expense 应清理 security_lots");
    assert_eq!(stx_after, 0, "buy→expense 应清理 security_transactions");
}

#[test]
fn delete_transaction_internal_cleans_up_buy_lots() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-inv", "inst-aapl");

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-aapl", 10.0, 1000000, 500),
    )
    .unwrap()
    .id;

    let lots: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lots, 1, "买入应建仓一个 lot");
    let stx: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stx, 1);

    delete_transaction_internal(&conn, &buy_id).unwrap();

    let lots_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lots_after, 0, "删除买入应清理 security_lots");
    let stx_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stx_after, 0, "删除买入应清理 security_transactions");
    let deleted: i64 = conn
        .query_row(
            "SELECT is_deleted FROM transactions WHERE id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted, 1, "交易应被软删除");
}

#[test]
fn delete_transaction_internal_cascades_active_sells_and_purges_buy_lots() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-casc", "inst-casc");

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-casc", "inst-casc", 10.0, 1000000, 0),
    )
    .unwrap()
    .id;
    let mut sell = make_buy_input("acc-casc", "inst-casc", 4.0, 1100000, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-01-20".into();
    let sell_id = create_transaction_internal(&conn, sell).unwrap().id;

    delete_transaction_internal(&conn, &buy_id).unwrap();

    let (buy_deleted, sell_deleted): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT is_deleted FROM transactions WHERE id=?1), \
                    (SELECT is_deleted FROM transactions WHERE id=?2)",
            params![buy_id, sell_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(buy_deleted, 1, "buy 应被软删除");
    assert_eq!(sell_deleted, 1, "在用 sell 应被级联软删除");

    let (lots, lot_sales, buy_stx, sell_stx): (i64, i64, i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1), \
                    (SELECT COUNT(*) FROM security_lot_sales WHERE lot_id IN \
                       (SELECT id FROM security_lots WHERE buy_transaction_id=?1)), \
                    (SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1 AND action='buy'), \
                    (SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?2 AND action='sell')",
            params![buy_id, sell_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(lots, 0, "级联后批次应整批清理");
    assert_eq!(lot_sales, 0, "级联后该批次的卖出匹配应清空");
    assert_eq!(buy_stx, 0, "buy 明细应清理");
    assert_eq!(sell_stx, 0, "级联 sell 的明细应随回补清理");

    let delete_ops: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sync_ops WHERE entity_id IN (?1, ?2) \
             AND payload LIKE '%\"delete\"%'",
            params![buy_id, sell_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(delete_ops, 2, "主删与级联删应各产出一条 delete op");
}

#[test]
fn delete_transaction_internal_sell_restores_holding() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-rev", "inst-rev");

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-rev", "inst-rev", 10.0, 1000000, 0),
    )
    .unwrap()
    .id;
    let mut sell = make_buy_input("acc-rev", "inst-rev", 4.0, 1100000, 0);
    sell.kind = TransactionKind::Sell;
    let sell_id = create_transaction_internal(&conn, sell).unwrap().id;

    delete_transaction_internal(&conn, &sell_id).unwrap();

    let (sell_deleted, remaining): (i64, f64) = conn
        .query_row(
            "SELECT (SELECT is_deleted FROM transactions WHERE id=?1), \
                    (SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?2)",
            params![sell_id, buy_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(sell_deleted, 1, "sell 应被软删除");
    assert_eq!(remaining, 10.0, "删除卖出应回补持仓扣减");

    let (lot_sales, sell_stx): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM security_lot_sales WHERE sell_transaction_id=?1), \
                    (SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1 AND action='sell')",
            params![sell_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(lot_sales, 0, "删除卖出应清空其卖出匹配");
    assert_eq!(sell_stx, 0, "删除卖出应清空其卖出明细");
}

#[test]
fn delete_buy_cascades_sell_spanning_multiple_buys_and_restores_sibling_lot() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-ripple", "inst-ripple");

    let buy_a = create_transaction_internal(
        &conn,
        make_buy_input("acc-ripple", "inst-ripple", 10.0, 1000000, 0),
    )
    .unwrap()
    .id;
    let mut buy_b_input = make_buy_input("acc-ripple", "inst-ripple", 10.0, 1050000, 0);
    buy_b_input.date = "2026-01-12".into();
    let buy_b = create_transaction_internal(&conn, buy_b_input).unwrap().id;
    let mut sell = make_buy_input("acc-ripple", "inst-ripple", 12.0, 1100000, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-01-20".into();
    let sell_id = create_transaction_internal(&conn, sell).unwrap().id;

    let matched_lots: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT lot_id) FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(matched_lots, 2, "卖出应跨两笔买入批次匹配");

    delete_transaction_internal(&conn, &buy_a).unwrap();

    let (buy_a_deleted, buy_b_deleted, sell_deleted): (i64, i64, i64) = conn
        .query_row(
            "SELECT (SELECT is_deleted FROM transactions WHERE id=?1), \
                    (SELECT is_deleted FROM transactions WHERE id=?2), \
                    (SELECT is_deleted FROM transactions WHERE id=?3)",
            params![buy_a, buy_b, sell_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(buy_a_deleted, 1);
    assert_eq!(sell_deleted, 1, "跨批次 sell 应被级联软删除");
    assert_eq!(buy_b_deleted, 0, "兄弟买入不应受牵连");

    let remaining_b: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining_b, 10.0, "级联回补应覆盖兄弟批次");
    let sales_on_b: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lot_sales WHERE lot_id IN \
             (SELECT id FROM security_lots WHERE buy_transaction_id=?1)",
            params![buy_b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sales_on_b, 0, "兄弟批次上不应残留卖出匹配");
}

#[test]
fn delete_buy_purges_ghost_lot_sales_of_deleted_sells() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-ghost", "inst-ghost");

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-ghost", "inst-ghost", 10.0, 1000000, 0),
    )
    .unwrap()
    .id;
    let mut sell = make_buy_input("acc-ghost", "inst-ghost", 4.0, 1100000, 0);
    sell.kind = TransactionKind::Sell;
    let sell_id = create_transaction_internal(&conn, sell).unwrap().id;
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![sell_id],
    )
    .unwrap();

    delete_transaction_internal(&conn, &buy_id).unwrap();

    let (buy_deleted, lots, ghost_lot_sales): (i64, i64, i64) = conn
        .query_row(
            "SELECT (SELECT is_deleted FROM transactions WHERE id=?1), \
                    (SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1), \
                    (SELECT COUNT(*) FROM security_lot_sales WHERE sell_transaction_id=?2)",
            params![buy_id, sell_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(buy_deleted, 1, "幽灵占用不再锁死删除");
    assert_eq!(lots, 0, "批次应整批清理");
    assert_eq!(ghost_lot_sales, 0, "幽灵匹配应随批次火化");
}

#[test]
fn update_buy_ignores_ghost_lot_sales_of_deleted_sells() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-ghost-upd", "inst-ghost-upd");

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-ghost-upd", "inst-ghost-upd", 10.0, 1000000, 0),
    )
    .unwrap()
    .id;
    let mut sell = make_buy_input("acc-ghost-upd", "inst-ghost-upd", 4.0, 1100000, 0);
    sell.kind = TransactionKind::Sell;
    let sell_id = create_transaction_internal(&conn, sell).unwrap().id;
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![sell_id],
    )
    .unwrap();

    update_transaction_internal(
        &conn,
        &buy_id,
        make_buy_input("acc-ghost-upd", "inst-ghost-upd", 5.0, 1200000, 0),
    )
    .unwrap();

    let (ghost_lot_sales, remaining): (i64, f64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM security_lot_sales WHERE sell_transaction_id=?1), \
                    (SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?2)",
            params![sell_id, buy_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(ghost_lot_sales, 0, "幽灵匹配应随批次重建消失");
    assert_eq!(remaining, 5.0, "批次应按新输入重建");
}

fn inject_soft_delete_failure(conn: &Connection) {
    conn.execute(
        "CREATE TRIGGER block_soft_delete BEFORE UPDATE ON transactions \
         BEGIN SELECT RAISE(ABORT, '测试注入：软删失败'); END",
        [],
    )
    .unwrap();
}

#[test]
fn delete_transaction_internal_rolls_back_lot_cleanup_when_soft_delete_fails() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-inv-rb", "inst-rb2");

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-inv-rb", "inst-rb2", 10.0, 1000000, 0),
    )
    .unwrap()
    .id;
    inject_soft_delete_failure(&conn);

    let err = delete_transaction_internal(&conn, &buy_id).unwrap_err();
    assert!(
        err.to_string().contains("测试注入：软删失败"),
        "应上抛注入的软删错误，实际: {err:?}"
    );

    let (deleted, lots, stx): (i64, i64, i64) = conn
        .query_row(
            "SELECT (SELECT is_deleted FROM transactions WHERE id=?1), \
                    (SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1), \
                    (SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1)",
            params![buy_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(deleted, 0, "软删 UPDATE 失败，交易不应被软删除");
    assert_eq!(lots, 1, "回滚后持仓批次不应残留删除");
    assert_eq!(stx, 1, "回滚后买卖明细不应残留删除");
}

#[test]
fn create_refund_linked_to_expense() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ref", "现金", "cash", "CNY", 0);

    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            funding: Vec::new(),
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: Some("acc-ref".into()),
            date: "2026-04-01".into(),
            category_id: None,
            to_account_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            funding_account_id: None,
            source_order_no: None,
            note: None,
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            idempotency_key: None,
            origin: None,
            fx_rate: None,
            purchases: Vec::new(),
        },
    )
    .unwrap()
    .id;

    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            funding: Vec::new(),
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Refund,
            amount_cents: 200,
            currency_code: "CNY".into(),
            account_id: Some("acc-ref".into()),
            date: "2026-04-05".into(),
            merchant_id: None,
            refund_of_transaction_id: Some(expense_id.clone()),
            funding_account_id: None,
            category_id: None,
            to_account_id: None,
            source_order_no: None,
            note: None,
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            idempotency_key: None,
            origin: None,
            fx_rate: None,
            purchases: Vec::new(),
        },
    )
    .unwrap()
    .id;

    let (kind, refund_of): (TransactionKind, Option<String>) = conn
        .query_row(
            "SELECT kind, refund_of_transaction_id FROM transactions WHERE id=?1",
            params![refund_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, TransactionKind::Refund);
    assert_eq!(refund_of, Some(expense_id));
}

#[test]
fn update_transaction_internal_replaces_fields_and_bumps_version() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-upd", "现金", "cash", "CNY", 0);
    let id = create_transaction_internal(
        &conn,
        make_input("acc-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap()
    .id;

    let mut edited = make_input("acc-upd", TransactionKind::Expense, 900, "2026-01-05");
    edited.note = Some("改后备注".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Expense);
    assert_eq!(t.amount_cents, 900);
    assert_eq!(t.date, "2026-01-05");
    assert_eq!(t.note.as_deref(), Some("改后备注"));
    assert_eq!(t.version, 2, "修改后版本号应递增");
}

#[test]
fn update_transaction_internal_returns_not_found_for_missing_or_deleted() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-upd", "现金", "cash", "CNY", 0);
    let id = create_transaction_internal(
        &conn,
        make_input("acc-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap()
    .id;

    let err = update_transaction_internal(
        &conn,
        "不存在的id",
        make_input("acc-upd", TransactionKind::Expense, 100, "2026-01-01"),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));

    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![id],
    )
    .unwrap();
    let err = update_transaction_internal(
        &conn,
        &id,
        make_input("acc-upd", TransactionKind::Expense, 100, "2026-01-01"),
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "已软删除应视为不存在"
    );
}

#[test]
fn update_transaction_internal_reuses_kind_validation_transfer_needs_target() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-upd", "现金", "cash", "CNY", 0);
    let id = create_transaction_internal(
        &conn,
        make_input("acc-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap()
    .id;

    let err = update_transaction_internal(
        &conn,
        &id,
        make_input("acc-upd", TransactionKind::Transfer, 1000, "2026-01-02"),
    )
    .unwrap_err();
    match err {
        AppError::Coded { message, .. } => assert!(message.contains("目标账户")),
        other => panic!("expected Coded, got {other:?}"),
    }
}

#[test]
fn update_transaction_internal_cross_kind_expense_to_transfer() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-upd-a", "A", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-upd-b", "B", "cash", "CNY", 0);
    let id = create_transaction_internal(
        &conn,
        make_input("acc-upd-a", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap()
    .id;

    let transfer = TransactionInput {
        policy_id: None,
        to_account_id: Some("acc-upd-b".into()),
        ..make_input("acc-upd-a", TransactionKind::Transfer, 1000, "2026-01-02")
    };
    update_transaction_internal(&conn, &id, transfer).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Transfer);
    assert_eq!(t.to_account_id.as_deref(), Some("acc-upd-b"));
}

#[test]
fn update_transaction_internal_buy_rebuilds_lot() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-inv", "inst-aapl");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-aapl", 10.0, 1000000, 500),
    )
    .unwrap()
    .id;

    let edited = make_buy_input("acc-inv", "inst-aapl", 5.0, 1200000, 0);
    update_transaction_internal(&conn, &buy_id, edited).unwrap();

    let t = get_transaction_internal(&conn, &buy_id).unwrap();
    assert_eq!(t.kind, TransactionKind::Buy);
    assert_eq!(t.amount_cents, 5 * 12000, "买入金额 = 数量×单价+费用");

    let (init, remaining): (f64, f64) = conn
        .query_row(
            "SELECT initial_quantity, remaining_quantity FROM security_lots \
             WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(init, 5.0, "应重建为新的持仓数量");
    assert_eq!(remaining, 5.0);
    let stx: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stx, 1, "重建后应有一条 security_transaction");
}

#[test]
fn update_transaction_internal_rejects_partially_sold_buy() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-inv2", "inst-msft");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-inv2", "inst-msft", 10.0, 1000000, 0),
    )
    .unwrap()
    .id;

    let mut sell = make_buy_input("acc-inv2", "inst-msft", 4.0, 1100000, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-01-20".into();
    create_transaction_internal(&conn, sell).unwrap();

    let err = update_transaction_internal(
        &conn,
        &buy_id,
        make_buy_input("acc-inv2", "inst-msft", 5.0, 1000000, 0),
    )
    .unwrap_err();
    match err {
        AppError::Coded { message, .. } => assert_eq!(message, "该买入交易已有部分卖出，无法修改"),
        other => panic!("expected Coded, got {other:?}"),
    }
}

#[test]
fn update_transaction_internal_sell_reverses_and_reapplies() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-inv3", "inst-tsla");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-inv3", "inst-tsla", 10.0, 1000000, 0),
    )
    .unwrap()
    .id;

    let mut sell1 = make_buy_input("acc-inv3", "inst-tsla", 4.0, 1100000, 0);
    sell1.kind = TransactionKind::Sell;
    let sell_id = create_transaction_internal(&conn, sell1).unwrap().id;

    let mut sell2 = make_buy_input("acc-inv3", "inst-tsla", 3.0, 1200000, 0);
    sell2.kind = TransactionKind::Sell;
    sell2.date = "2026-02-01".into();
    update_transaction_internal(&conn, &sell_id, sell2).unwrap();

    let t = get_transaction_internal(&conn, &sell_id).unwrap();
    assert_eq!(t.kind, TransactionKind::Sell);
    assert_eq!(t.amount_cents, 3 * 12000, "卖出收入 = 数量×单价");

    let remaining: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 7.0, "修改卖出后持仓应反映新数量");

    let sales: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sales, 1);
}

fn inject_buy_lot_failure(conn: &Connection) {
    conn.execute(
        "CREATE TRIGGER block_buy_lot BEFORE INSERT ON security_lots \
         BEGIN SELECT RAISE(ABORT, '测试注入：建仓失败'); END",
        [],
    )
    .unwrap();
}

fn assert_no_creation_residue(conn: &Connection) {
    let (txns, stx, lots): (i64, i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM transactions), \
                    (SELECT COUNT(*) FROM security_transactions), \
                    (SELECT COUNT(*) FROM security_lots)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(txns, 0, "交易行不应残留");
    assert_eq!(stx, 0, "买卖明细不应残留");
    assert_eq!(lots, 0, "持仓批次不应残留");
}

#[test]
fn create_buy_mid_apply_failure_rolls_back_all() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-rb", "inst-rb");
    inject_buy_lot_failure(&conn);

    let err =
        create_transaction_internal(&conn, make_buy_input("acc-rb", "inst-rb", 10.0, 1000000, 0))
            .unwrap_err();
    assert!(
        err.to_string().contains("测试注入：建仓失败"),
        "应上抛注入的建仓错误，实际: {err:?}"
    );
    assert_no_creation_residue(&conn);
}

#[test]
fn create_nested_mode_leaves_rollback_ownership_to_outer_holder() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-n1", "现金", "cash", "CNY", 0);

    conn.execute("BEGIN", []).unwrap();
    let id = create_transaction_internal(
        &conn,
        make_input("acc-n1", TransactionKind::Income, 1000, "2026-01-01"),
    )
    .expect("嵌套模式下成功创建应直接返回 id");
    conn.execute("ROLLBACK", []).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "嵌套模式的提交权在外层：外层回滚后行应消失");
    let _ = id;

    conn.execute("BEGIN", []).unwrap();
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('outer-row','income',1,'CNY',1,'acc-n1','2026-01-01',?1,?1,1,'test',0)",
        params![test_support::FIXED_NOW],
    )
    .unwrap();
    let err = create_transaction_internal(
        &conn,
        make_input("acc-n1", TransactionKind::Expense, 0, "2026-01-02"),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("金额必须大于 0"),
        "嵌套模式失败应直接返回业务错误，实际: {err:?}"
    );
    let still_open: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(still_open, 1, "嵌套失败不应拖垮外层已写的行");
    conn.execute("ROLLBACK", []).unwrap();
}

#[test]
fn update_nested_mode_leaves_rollback_ownership_to_outer_holder() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-un1", "现金", "cash", "CNY", 0);
    let id = create_transaction_internal(
        &conn,
        make_input("acc-un1", TransactionKind::Income, 1000, "2026-01-01"),
    )
    .unwrap()
    .id;

    conn.execute("BEGIN", []).unwrap();
    update_transaction_internal(
        &conn,
        &id,
        make_input("acc-un1", TransactionKind::Income, 2000, "2026-01-01"),
    )
    .expect("嵌套模式下成功修改应直接返回");
    conn.execute("ROLLBACK", []).unwrap();
    let amount: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount, 1000, "嵌套模式的提交权在外层：外层回滚后修改应消失");

    conn.execute("BEGIN", []).unwrap();
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('outer-row-u','income',1,'CNY',1,'acc-un1','2026-01-01',?1,?1,1,'test',0)",
        params![test_support::FIXED_NOW],
    )
    .unwrap();
    let err = update_transaction_internal(
        &conn,
        &id,
        make_input("acc-un1", TransactionKind::Expense, 0, "2026-01-02"),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("金额必须大于 0"),
        "嵌套模式失败应直接返回业务错误，实际: {err:?}"
    );
    let still_open: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(still_open, 2, "嵌套失败不应拖垮外层已写的行");
    conn.execute("ROLLBACK", []).unwrap();
}

fn hkd_expense_input(account_id: &str, amount_cents: i64, date: &str) -> TransactionInput {
    TransactionInput {
        currency_code: "HKD".into(),
        ..make_input(account_id, TransactionKind::Expense, amount_cents, date)
    }
}

fn seeded_hkd_expense(conn: &Connection, date: &str) -> String {
    test_support::seed_account(conn, "acc-fx-ed", "港币户", "cash", "HKD", 0);
    test_support::seed_fx_rate_history(conn, "fxh-ed-w1", "HKD", "CNY", "2026-06-29", 0.9);
    create_transaction_internal(conn, hkd_expense_input("acc-fx-ed", 1000, date))
        .unwrap()
        .id
}

#[test]
fn update_note_only_reuses_inline_fx_when_series_missing() {
    let conn = test_support::open();
    let id = seeded_hkd_expense(&conn, "2026-07-01");
    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let mut edited = hkd_expense_input("acc-fx-ed", 1000, "2026-07-01");
    edited.note = Some("只改备注".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.amount_native_cents, 900, "本位币金额逐位不变");
    assert_eq!(t.fx_rate_used, Some(0.9), "留痕汇率逐位不变");
    assert_eq!(t.fx_rate_source, Some(FxRateSource::Series), "来源不变");
    assert_eq!(t.note.as_deref(), Some("只改备注"));
    assert_eq!(t.version, 2, "修改仍正常递增版本");
}

#[test]
fn update_category_only_reuses_inline_fx_when_series_missing() {
    let conn = test_support::open();
    let id = seeded_hkd_expense(&conn, "2026-07-01");
    let cat_fx_ed = seed_category(&conn, "交通", "expense");
    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let mut edited = hkd_expense_input("acc-fx-ed", 1000, "2026-07-01");
    edited.category_id = Some(cat_fx_ed);
    update_transaction_internal(&conn, &id, edited).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.amount_native_cents, 900, "本位币金额逐位不变");
    assert_eq!(t.fx_rate_used, Some(0.9));
    assert_eq!(t.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn update_merchant_only_reuses_inline_fx_when_series_missing() {
    let conn = test_support::open();
    let id = seeded_hkd_expense(&conn, "2026-07-01");
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('mer-fx-ed','便利店',?1,?1,1,'test',0)",
        params![test_support::FIXED_NOW],
    )
    .unwrap();
    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let mut edited = hkd_expense_input("acc-fx-ed", 1000, "2026-07-01");
    edited.merchant_id = Some("mer-fx-ed".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.amount_native_cents, 900, "本位币金额逐位不变");
    assert_eq!(t.fx_rate_used, Some(0.9));
    assert_eq!(t.fx_rate_source, Some(FxRateSource::Series));
    assert_eq!(t.merchant_id.as_deref(), Some("mer-fx-ed"), "商户已改");
}

#[test]
fn update_amount_changed_fails_coded_when_series_missing() {
    let conn = test_support::open();
    let id = seeded_hkd_expense(&conn, "2026-07-01");
    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let err = update_transaction_internal(
        &conn,
        &id,
        hkd_expense_input("acc-fx-ed", 2000, "2026-07-01"),
    )
    .unwrap_err();
    match err {
        AppError::Coded { code, .. } => assert_eq!(code, "fx.rate-missing"),
        other => panic!("应为 fx.rate-missing 码化错误，实际: {other:?}"),
    }
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.amount_cents, 1000, "失败行保持原值");
    assert_eq!(t.amount_native_cents, 900);
    assert_eq!(t.fx_rate_used, Some(0.9));
    assert_eq!(t.version, 1, "失败不落库");
}

#[test]
fn update_date_changed_requeries_new_week() {
    let conn = test_support::open();
    let id = seeded_hkd_expense(&conn, "2026-07-01");
    test_support::seed_fx_rate_history(&conn, "fxh-ed-w2", "HKD", "CNY", "2026-07-06", 0.95);

    update_transaction_internal(
        &conn,
        &id,
        hkd_expense_input("acc-fx-ed", 1000, "2026-07-08"),
    )
    .unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.date, "2026-07-08");
    assert_eq!(t.amount_native_cents, 950, "按新周汇率重算");
    assert_eq!(t.fx_rate_used, Some(0.95), "留痕更新为新周使用值");
    assert_eq!(t.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn update_currency_changed_requeries() {
    let conn = test_support::open();
    let id = seeded_hkd_expense(&conn, "2026-07-01");
    test_support::seed_account(&conn, "acc-usd-ed", "美元户", "cash", "USD", 0);
    test_support::seed_fx_rate_history(&conn, "fxh-ed-usd", "USD", "CNY", "2026-06-29", 7.2);

    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            currency_code: "USD".into(),
            account_id: Some("acc-usd-ed".into()),
            ..make_input("acc-usd-ed", TransactionKind::Expense, 1000, "2026-07-01")
        },
    )
    .unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.currency_code, "USD", "币种改为 USD 后按新币种折算");
    assert_eq!(t.amount_native_cents, 7200, "1000 × 7.2 按新币种序列点重查");
    assert_eq!(t.fx_rate_used, Some(7.2), "留痕更新为新币种使用值");
    assert_eq!(t.fx_rate_source, Some(FxRateSource::Series));
}

fn seeded_hkd_investment(conn: &Connection, instruments: &[&str]) {
    test_support::seed_account(conn, "acc-inv-fx", "港币投资户", "investment", "HKD", 0);
    test_support::seed_fx_rate_history(conn, "fxh-inv-w1", "HKD", "CNY", "2026-06-29", 0.9);
    for inst in instruments {
        test_support::seed_instrument(conn, inst, inst, &format!("标的{inst}"), "HKD", "hk");
    }
}

fn hkd_buy_input(instrument_id: &str, qty: f64, price: i64, fee: i64) -> TransactionInput {
    TransactionInput {
        currency_code: "HKD".into(),
        date: "2026-07-01".into(),
        ..make_buy_input("acc-inv-fx", instrument_id, qty, price, fee)
    }
}

fn hkd_dividend_input(instrument_id: &str, amount_cents: i64) -> TransactionInput {
    TransactionInput {
        currency_code: "HKD".into(),
        instrument_id: Some(instrument_id.into()),
        ..make_input(
            "acc-inv-fx",
            TransactionKind::Dividend,
            amount_cents,
            "2026-07-01",
        )
    }
}

#[test]
fn update_buy_note_only_reuses_inline_fx_when_series_missing() {
    let conn = test_support::open();
    seeded_hkd_investment(&conn, &["inst-fx-b"]);
    let id = create_transaction_internal(&conn, hkd_buy_input("inst-fx-b", 10.0, 100000, 100))
        .unwrap()
        .id;
    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let mut edited = hkd_buy_input("inst-fx-b", 10.0, 100000, 100);
    edited.note = Some("只改备注".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.amount_cents, 10100, "金额 = 数量×单价+费用");
    assert_eq!(t.amount_native_cents, 9090, "本位币金额逐位不变");
    assert_eq!(t.fx_rate_used, Some(0.9), "留痕汇率逐位不变");
    assert_eq!(t.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn update_sell_note_only_reuses_inline_fx_when_series_missing() {
    let conn = test_support::open();
    seeded_hkd_investment(&conn, &["inst-fx-s"]);
    create_transaction_internal(&conn, hkd_buy_input("inst-fx-s", 10.0, 100000, 0)).unwrap();
    let mut sell = hkd_buy_input("inst-fx-s", 4.0, 100000, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-07-02".into();
    let sell_id = create_transaction_internal(&conn, sell).unwrap().id;
    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let mut edited = hkd_buy_input("inst-fx-s", 4.0, 100000, 0);
    edited.kind = TransactionKind::Sell;
    edited.date = "2026-07-02".into();
    edited.note = Some("只改备注".into());
    update_transaction_internal(&conn, &sell_id, edited).unwrap();

    let t = get_transaction_internal(&conn, &sell_id).unwrap();
    assert_eq!(t.amount_cents, 4000);
    assert_eq!(t.amount_native_cents, 3600, "本位币金额逐位不变");
    assert_eq!(t.fx_rate_used, Some(0.9));
    assert_eq!(t.fx_rate_source, Some(FxRateSource::Series));
    assert_eq!(t.version, 2);
}

#[test]
fn update_dividend_note_only_reuses_inline_fx_when_series_missing() {
    let conn = test_support::open();
    seeded_hkd_investment(&conn, &["inst-fx-d"]);
    let id = create_transaction_internal(&conn, hkd_dividend_input("inst-fx-d", 6000))
        .unwrap()
        .id;
    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let mut edited = hkd_dividend_input("inst-fx-d", 6000);
    edited.note = Some("只改备注".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.amount_native_cents, 5400, "本位币金额逐位不变");
    assert_eq!(t.fx_rate_used, Some(0.9));
    assert_eq!(t.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn update_convert_note_only_reuses_inline_fx_when_series_missing() {
    let conn = test_support::open();
    seeded_hkd_investment(&conn, &["inst-fx-co", "inst-fx-ci"]);
    create_transaction_internal(&conn, hkd_buy_input("inst-fx-co", 10.0, 100000, 0)).unwrap();
    let mut convert = hkd_buy_input("inst-fx-co", 10.0, 0, 0);
    convert.kind = TransactionKind::Convert;
    convert.instrument_id = Some("inst-fx-co".into());
    convert.to_instrument_id = Some("inst-fx-ci".into());
    convert.to_quantity = Some(10.0);
    convert.out_amount_cents = Some(11000);
    convert.in_amount_cents = Some(11000);
    convert.price_cents = None;
    convert.date = "2026-07-03".into();
    let convert_id = create_transaction_internal(&conn, convert).unwrap().id;
    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let mut edited = hkd_buy_input("inst-fx-co", 10.0, 0, 0);
    edited.kind = TransactionKind::Convert;
    edited.instrument_id = Some("inst-fx-co".into());
    edited.to_instrument_id = Some("inst-fx-ci".into());
    edited.to_quantity = Some(10.0);
    edited.out_amount_cents = Some(11000);
    edited.in_amount_cents = Some(11000);
    edited.price_cents = None;
    edited.date = "2026-07-03".into();
    edited.note = Some("只改备注".into());
    update_transaction_internal(&conn, &convert_id, edited).unwrap();

    let t = get_transaction_internal(&conn, &convert_id).unwrap();
    assert_eq!(t.amount_cents, 10000, "结转成本锚点重算不变");
    assert_eq!(t.amount_native_cents, 9000, "本位币金额逐位不变");
    assert_eq!(t.fx_rate_used, Some(0.9));
    assert_eq!(t.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn update_with_explicit_fx_rate_beats_baseline_reuse() {
    let conn = test_support::open();
    let id = seeded_hkd_expense(&conn, "2026-07-01");
    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let mut edited = hkd_expense_input("acc-fx-ed", 1000, "2026-07-01");
    edited.fx_rate = Some(0.88);
    update_transaction_internal(&conn, &id, edited).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.amount_native_cents, 880, "按显式给定值折算");
    assert_eq!(t.fx_rate_used, Some(0.88));
    assert_eq!(
        t.fx_rate_source,
        Some(FxRateSource::Explicit),
        "来源改标显式"
    );
}

#[test]
fn update_currency_guard_rejects_dirty_row_until_made_consistent() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-dirty", "现金", "cash", "CNY", 0);
    let dirty = NormalizedRow {
        funding: Vec::new(),
        kind: TransactionKind::Expense,
        amount_cents: 1000,
        currency_code: "USD".into(),
        amount_native_cents: 1000,
        fx_rate_used: None,
        fx_rate_source: None,
        account_id: Some("acc-dirty".into()),
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
    let id = insert_row(&conn, &dirty).unwrap();

    let mut keep_dirty = make_input("acc-dirty", TransactionKind::Expense, 1000, "2026-01-01");
    keep_dirty.currency_code = "USD".into();
    keep_dirty.note = Some("只改备注".into());
    let err = update_transaction_internal(&conn, &id, keep_dirty).unwrap_err();
    match err {
        AppError::Coded { code, params, .. } => {
            assert_eq!(code, "transaction.currency-mismatch");
            assert_eq!(params, vec!["CNY".to_string(), "USD".to_string()]);
        }
        other => panic!("应为码化错误，实际: {other:?}"),
    }
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.note, None, "失败不落库");

    let mut fixed = make_input("acc-dirty", TransactionKind::Expense, 1000, "2026-01-01");
    fixed.note = Some("只改备注".into());
    update_transaction_internal(&conn, &id, fixed).unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.currency_code, "CNY");
    assert_eq!(t.note.as_deref(), Some("只改备注"));
}
