//! 写删行为：创建 / 软删 / 修改、refund 链、dividend / split「暂不支持」拒绝、
//! 买入卖出副作用清理，以及行为层编排入口（嵌套感知事务，issue #228 / #229 / ADR-0033）。

use super::super::*;
use super::common::{insert_account, make_buy_input, make_input, setup, setup_investment_account};
use crate::error::ErrClass;
use rusqlite::Connection;

use crate::db::{device_id, now_iso};
use crate::transaction::amount::TransactionKind;
use rusqlite::params;

#[test]
fn create_income_and_expense_transactions() {
    let conn = setup();
    insert_account(&conn, "acc-crud", "现金", "cash", "CNY");

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
    let conn = setup();
    insert_account(&conn, "acc-from", "A账户", "cash", "CNY");
    insert_account(&conn, "acc-to", "B账户", "cash", "CNY");

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Transfer,
            amount_cents: 3000,
            currency_code: "CNY".into(),
            account_id: "acc-from".into(),
            to_account_id: Some("acc-to".into()),
            date: "2026-03-01".into(),
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            note: None,
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
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
    let conn = setup();
    insert_account(&conn, "acc-del", "现金", "cash", "CNY");

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
        params![id, now_iso(), device_id()],
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
    let conn = setup();
    insert_account(&conn, "acc-missing", "现金", "cash", "CNY");

    let err = delete_transaction_internal(&conn, "不存在的id").unwrap_err();
    match err {
        AppError::Coded { message, .. } => assert!(message.contains("交易不存在")),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn delete_transaction_internal_returns_not_found_for_already_deleted() {
    let conn = setup();
    insert_account(&conn, "acc-gone", "现金", "cash", "CNY");
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

// ---------------------------------------------------------------------------
// issue #72：dividend / split 显式「暂不支持」拒绝
// ---------------------------------------------------------------------------
/// dividend/split 已声明但未实现：经交易创建接口显式「暂不支持」拒绝。
/// 此前经交易接口创建 dividend/split 落入 writer::normalize 的通用兜底，返回语义不明的
/// 「仅处理通用交易类型」；现改为明确的「暂不支持」——两者均不落库（见 spec #69）。
#[test]
fn create_transaction_internal_rejects_dividend_and_split_with_not_supported() {
    let conn = setup();
    insert_account(&conn, "acc-unsup", "现金", "cash", "CNY");

    for (kind, amount) in [(TransactionKind::Dividend, 60), (TransactionKind::Split, 0)] {
        let err =
            create_transaction_internal(&conn, make_input("acc-unsup", kind, amount, "2026-05-04"))
                .unwrap_err();
        match err {
            AppError::Coded { message, .. } => assert!(
                message.contains("暂不支持"),
                "{kind} 应报「暂不支持」，实际: {message}"
            ),
            other => panic!("expected Coded, got {other:?}"),
        }
    }

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "拒绝的交易不应落库");
}

/// 修改为 dividend/split 同样经行为层显式拒绝（单点分派覆盖创建与修改，事务回滚）。
#[test]
fn update_transaction_rejects_dividend_and_split_with_not_supported() {
    let conn = setup();
    insert_account(&conn, "acc-unsup-upd", "现金", "cash", "CNY");
    let id = create_transaction_internal(
        &conn,
        make_input("acc-unsup-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap()
    .id;

    for (kind, amount) in [(TransactionKind::Dividend, 60), (TransactionKind::Split, 0)] {
        let err = update_transaction_internal(
            &conn,
            &id,
            make_input("acc-unsup-upd", kind, amount, "2026-05-04"),
        )
        .unwrap_err();
        match err {
            AppError::Coded { message, .. } => assert!(
                message.contains("暂不支持"),
                "{kind} 应报「暂不支持」，实际: {message}"
            ),
            other => panic!("expected Coded, got {other:?}"),
        }
        // 修改被拒绝后原交易保持不变（事务回滚）。
        let t = get_transaction_internal(&conn, &id).unwrap();
        assert_eq!(t.kind, TransactionKind::Expense);
        assert_eq!(t.amount_cents, 500);
    }
}

/// 跨 kind 修改经行为层原子清理并重建副作用（spec #69 故事 13）：
/// expense→buy 建仓、buy→expense 清理，均不留孤儿持仓关联。
#[test]
fn update_transaction_cross_kind_rebuilds_side_effects_atomically() {
    let conn = setup();
    insert_account(&conn, "acc-cash-x", "现金", "cash", "CNY");
    setup_investment_account(&conn, "acc-x", "inst-x");

    // expense → buy：应建仓 lot。
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

    // buy → expense：应清理持仓关联，无孤儿 lot / security_transaction。
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
    let conn = setup();
    setup_investment_account(&conn, "acc-inv", "inst-aapl");

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
fn delete_transaction_internal_rejects_partially_sold_buy() {
    let conn = setup();
    setup_investment_account(&conn, "acc-inv2", "inst-msft");

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

    let err = delete_transaction_internal(&conn, &buy_id).unwrap_err();
    // 守卫文案按入口内化（ADR-0033 决策 #4）：删除入口固定返回自己的措辞，
    // 与修改入口对同一守卫各持措辞、互不漂移。
    match err {
        AppError::Coded { message, .. } => assert_eq!(message, "该买入交易已有部分卖出，无法删除"),
        other => panic!("expected Coded, got {other:?}"),
    }
}

/// 注入「软删中途失败」：行为层 revert（清理持仓批次）成功后、软删 UPDATE 被
/// 触发器 RAISE(ABORT) 挡下——纯测试侧手段（spec #169 定案），产品代码零 hook。
fn inject_soft_delete_failure(conn: &Connection) {
    conn.execute(
        "CREATE TRIGGER block_soft_delete BEFORE UPDATE ON transactions \
         BEGIN SELECT RAISE(ABORT, '测试注入：软删失败'); END",
        [],
    )
    .unwrap();
}

/// 删除路径事务缺口修复（issue #229 / ADR-0033 决策 #3）：revert（清理持仓批次）
/// 与软删 UPDATE 纳入同一事务，软删中途失败整体回滚——不再出现
/// 「持仓已删而交易仍在」的中间态，报错返回。
#[test]
fn delete_transaction_internal_rolls_back_lot_cleanup_when_soft_delete_fails() {
    let conn = setup();
    setup_investment_account(&conn, "acc-inv-rb", "inst-rb2");

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

    // 数据终态：交易未被软删，持仓批次与买卖明细原样保留。
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
    let conn = setup();
    insert_account(&conn, "acc-ref", "现金", "cash", "CNY");

    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: "acc-ref".into(),
            date: "2026-04-01".into(),
            category_id: None,
            to_account_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            note: None,
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap()
    .id;

    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Refund,
            amount_cents: 200,
            currency_code: "CNY".into(),
            account_id: "acc-ref".into(),
            date: "2026-04-05".into(),
            merchant_id: None,
            refund_of_transaction_id: Some(expense_id.clone()),
            category_id: None,
            to_account_id: None,
            note: None,
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
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
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
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
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
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
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
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
    let conn = setup();
    insert_account(&conn, "acc-upd-a", "A", "cash", "CNY");
    insert_account(&conn, "acc-upd-b", "B", "cash", "CNY");
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
    let conn = setup();
    setup_investment_account(&conn, "acc-inv", "inst-aapl");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-aapl", 10.0, 1000000, 500),
    )
    .unwrap()
    .id;

    // 编辑买入：数量/单价变化，应重建 lot 与 security_transaction。
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
    let conn = setup();
    setup_investment_account(&conn, "acc-inv2", "inst-msft");
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
    // 守卫文案按入口内化（ADR-0033 决策 #4）：修改入口固定返回自己的措辞。
    match err {
        AppError::Coded { message, .. } => assert_eq!(message, "该买入交易已有部分卖出，无法修改"),
        other => panic!("expected Coded, got {other:?}"),
    }
}

#[test]
fn update_transaction_internal_sell_reverses_and_reapplies() {
    let conn = setup();
    setup_investment_account(&conn, "acc-inv3", "inst-tsla");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-inv3", "inst-tsla", 10.0, 1000000, 0),
    )
    .unwrap()
    .id;

    let mut sell1 = make_buy_input("acc-inv3", "inst-tsla", 4.0, 1100000, 0);
    sell1.kind = TransactionKind::Sell;
    let sell_id = create_transaction_internal(&conn, sell1).unwrap().id;

    // 编辑卖出：数量 4→3、单价上涨。应先回补旧扣减再按新输入重新匹配。
    let mut sell2 = make_buy_input("acc-inv3", "inst-tsla", 3.0, 1200000, 0);
    sell2.kind = TransactionKind::Sell;
    sell2.date = "2026-02-01".into();
    update_transaction_internal(&conn, &sell_id, sell2).unwrap();

    let t = get_transaction_internal(&conn, &sell_id).unwrap();
    assert_eq!(t.kind, TransactionKind::Sell);
    assert_eq!(t.amount_cents, 3 * 12000, "卖出收入 = 数量×单价");

    // 修改卖出后持仓剩余 = 10 - 3 = 7。
    let remaining: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 7.0, "修改卖出后持仓应反映新数量");

    // 旧卖出关联已清空，重建为一条新的。
    let sales: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sales, 1);
}

// ---------------------------------------------------------------------------
// 行为层 create 编排入口（issue #228 / ADR-0033）：嵌套感知事务
// ---------------------------------------------------------------------------
/// 注入「建仓中途失败」：security_transactions 已写入、security_lots 写入时被
/// 触发器 RAISE(ABORT) 挡下——纯测试侧手段（spec #169 定案），产品代码零 hook。
fn inject_buy_lot_failure(conn: &Connection) {
    conn.execute(
        "CREATE TRIGGER block_buy_lot BEFORE INSERT ON security_lots \
         BEGIN SELECT RAISE(ABORT, '测试注入：建仓失败'); END",
        [],
    )
    .unwrap();
}

/// 断言无任何残留：交易行、买卖明细、持仓批次均为 0（数据终态，外部可观察）。
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

/// 创建买入 apply 中途失败 → 交易行与持仓副作用均无残留，报错返回
/// （issue #228 验收：自持事务整体回滚，修复创建路径中间态缺口）。
#[test]
fn create_buy_mid_apply_failure_rolls_back_all() {
    let conn = setup();
    setup_investment_account(&conn, "acc-rb", "inst-rb");
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

/// 嵌套模式（外层批次事务中）：create 加入外层、失败直接返回错误，回滚归外层持有者
/// ——Ok 不提交（外层 ROLLBACK 即消失）、Err 不回滚外层已写的行。
#[test]
fn create_nested_mode_leaves_rollback_ownership_to_outer_holder() {
    let conn = setup();
    insert_account(&conn, "acc-n1", "现金", "cash", "CNY");

    // 加入外层：Ok 不自持 COMMIT——外层 ROLLBACK 仍能成功、回滚后行消失，
    // 证明提交点归外层持有者（若 create 自作主张提交，ROLLBACK 会因无活动事务报错）。
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

    // 加入外层：Err 直接返回错误、不回滚外层——外层已写的行保留，去留由外层决定。
    conn.execute("BEGIN", []).unwrap();
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('outer-row','income',1,'CNY',1,'acc-n1','2026-01-01','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        [],
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
    // 外层事务仍开启且已写行仍在（若嵌套失败自行回滚外层，此计数会归零）：
    let still_open: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(still_open, 1, "嵌套失败不应拖垮外层已写的行");
    conn.execute("ROLLBACK", []).unwrap();
}

/// 嵌套模式（外层事务中）：update 加入外层、失败直接返回错误，回滚归外层持有者
/// ——Ok 不提交（外层 ROLLBACK 即消失）、Err 不回滚外层已写的行。
#[test]
fn update_nested_mode_leaves_rollback_ownership_to_outer_holder() {
    let conn = setup();
    insert_account(&conn, "acc-un1", "现金", "cash", "CNY");
    let id = create_transaction_internal(
        &conn,
        make_input("acc-un1", TransactionKind::Income, 1000, "2026-01-01"),
    )
    .unwrap()
    .id;

    // 加入外层：Ok 不自持 COMMIT——外层 ROLLBACK 后修改消失，
    // 证明提交点归外层持有者（若 update 自作主张提交，ROLLBACK 会因无活动事务报错）。
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

    // 加入外层：Err 直接返回错误、不回滚外层——外层已写的行保留，去留由外层决定。
    conn.execute("BEGIN", []).unwrap();
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('outer-row-u','income',1,'CNY',1,'acc-un1','2026-01-01','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        [],
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
    // 外层事务仍开启且已写行仍在（若嵌套失败自行回滚外层，此计数会归零）：
    let still_open: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(still_open, 2, "嵌套失败不应拖垮外层已写的行");
    conn.execute("ROLLBACK", []).unwrap();
}
