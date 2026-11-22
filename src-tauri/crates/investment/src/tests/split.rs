use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{
    create_transaction_internal, delete_transaction_internal, update_transaction_internal,
};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_fx_history_weeks, seed_instrument};

fn balance_snapshot(conn: &rusqlite::Connection) -> Vec<(String, i64)> {
    let mut rows: Vec<(String, i64)> =
        ledger_accounts::balance::list_accounts_with_visibility(conn, true)
            .unwrap()
            .into_iter()
            .map(|account| {
                let balance = ledger_accounts::balance::compute_balance(conn, &account.id).unwrap();
                (account.id, balance)
            })
            .collect();
    rows.sort();
    rows
}

fn seed_split_scene(conn: &rusqlite::Connection) {
    seed_account(conn, "acc-sp", "股票户", "investment", "CNY", 0);
    seed_instrument(conn, "inst-sp", "502010", "证券基金", "CNY", "unknown");
}

fn lots_of(conn: &rusqlite::Connection, instrument_id: &str) -> Vec<(f64, f64, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT initial_quantity, remaining_quantity, cost_per_unit_cents \
             FROM security_lots WHERE instrument_id=?1 ORDER BY rowid",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![instrument_id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

fn realized_pnl_of(conn: &rusqlite::Connection, instrument_id: &str) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(s.realized_pnl_cents),0) FROM security_lot_sales s \
         JOIN security_lots l ON l.id = s.lot_id WHERE l.instrument_id=?1",
        rusqlite::params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn split_partial_sell_settles_at_restated_cost() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();

    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 10.0)).unwrap();

    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots.len(), 1);
    assert!((lots[0].1 - 110.0).abs() < 1e-9, "剩余数量 × f = 110");
    assert_eq!(
        lots[0].2, 90_909,
        "每份成本 = round(100000 ÷ 1.1)，尾差闭合"
    );

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            55.0,
            100_000,
            "2026-03-01",
        ),
    )
    .unwrap();
    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        5_000,
        "部分卖出按重述后每份成本结算：550 − 55×9.0909 = 50 元"
    );

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            55.0,
            100_000,
            "2026-04-01",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 10_000);
}

#[test]
fn split_row_has_no_fx_rate_trace() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 10.0))
        .unwrap()
        .id;

    let (fx_rate_used, fx_rate_source): (Option<f64>, Option<String>) = conn
        .query_row(
            "SELECT fx_rate_used, fx_rate_source FROM transactions WHERE id=?1",
            rusqlite::params![split_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(fx_rate_used, None, "无现金腿不折算：无汇率留痕");
    assert_eq!(fx_rate_source, None, "无现金腿不折算：无来源留痕");
}

#[test]
fn split_tail_difference_lands_on_last_batch() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            200.0,
            20_000,
            "2026-01-15",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            10_000,
            "2026-01-20",
        ),
    )
    .unwrap();

    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 30.0))
        .unwrap()
        .id;

    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots.len(), 2);
    assert!((lots[0].1 - 60.0 * 290.0 / 260.0).abs() < 1e-9);
    assert_eq!(lots[0].2, 8_966, "非末批次独立取整稀释 round(10000 ÷ f)");
    assert!(
        (lots[1].0 - 200.0 * 290.0 / 260.0).abs() < 1e-9,
        "末批次初始数量同比例放大"
    );

    let canonical: i64 = conn
        .query_row(
            "SELECT CAST(COALESCE(SUM(t.amount_cents),0) - COALESCE((\
               SELECT SUM(CAST(ROUND(q * cpu / 100.0) AS INTEGER)) FROM (\
                 SELECT s.quantity AS q, s.cost_per_unit_cents AS cpu FROM security_lot_sales s \
                 JOIN security_lots l ON l.id = s.lot_id WHERE l.instrument_id='inst-sp' \
                 UNION ALL \
                 SELECT c.quantity, c.cost_per_unit_cents FROM security_lot_conversions c \
                 JOIN security_lots l ON l.id = c.lot_id WHERE l.instrument_id='inst-sp')),0) AS INTEGER) \
             FROM security_lots l JOIN transactions t ON t.id = l.buy_transaction_id \
             WHERE l.instrument_id='inst-sp' AND l.remaining_quantity > 0",
        [],
        |r| r.get(0),
    )
    .unwrap();
    assert_eq!(canonical, 46_000, "Σ 权威批次剩余成本精确不变");

    let audits: Vec<(f64, f64, f64, f64, i64, i64)> = {
        let mut stmt = conn
            .prepare(
                "SELECT initial_quantity_before, initial_quantity_after, \
                 remaining_quantity_before, remaining_quantity_after, \
                 cost_per_unit_cents_before, cost_per_unit_cents_after \
                 FROM security_lot_adjustments WHERE transaction_id=?1 ORDER BY lot_id",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![split_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    assert_eq!(audits.len(), 2, "每个在用批次一行审计");
    let mut audits = audits;
    audits.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
    assert_eq!(audits[0].2, 60.0, "批次 1 重述前剩余 60");
    assert_eq!(audits[0].4, 10_000, "批次 1 重述前每份成本 10000");
    assert!(
        (audits[1].3 - 200.0 * 290.0 / 260.0).abs() < 1e-9,
        "末批次审计 after 初始数量 = initial × f"
    );
    assert_eq!(
        audits[1].5, lots[1].2,
        "末批次审计 after 每份成本与落库值一致"
    );

    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        0,
        "split 不产生已实现盈亏"
    );
}

#[test]
fn split_does_not_break_full_liquidation_identity() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            20_000,
            "2026-01-20",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 60.0)).unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            120.0,
            5_000,
            "2026-02-20",
        ),
    )
    .unwrap();
    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        4_000,
        "Σ已实现 = Σ卖出(14000) − Σ买入(10000)，split 不引入新项"
    );
}

#[test]
fn consumption_recalc_without_split_matches_legacy_exact_values() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            20_000,
            "2026-01-20",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 4_000);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            60.0,
            5_000,
            "2026-02-20",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 1_000);
}

#[test]
fn holdings_as_of_includes_split_leg() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 33.976)).unwrap();

    let today = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-02-01").unwrap();
    assert!(
        (today - 133.976).abs() < 1e-6,
        "as-of 应含 split 腿：100 + 33.976，实际 {today}"
    );
    let before = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-01-31").unwrap();
    assert!((before - 100.0).abs() < 1e-6);
}

#[test]
fn split_reverse_shrinks_holding_and_keeps_total_cost() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let balances_before = balance_snapshot(&conn);

    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", -20.0)).unwrap();

    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots.len(), 1);
    assert!((lots[0].0 - 80.0).abs() < 1e-9, "初始数量 × f = 80");
    assert!((lots[0].1 - 80.0).abs() < 1e-9, "剩余数量 × f = 80");
    assert_eq!(
        lots[0].2, 125_000,
        "每份成本 = 100000 ÷ 0.8，批次总成本精确不变"
    );

    let (qty, cost_basis): (f64, i64) = conn
        .query_row(
            "SELECT quantity, cost_basis_cents FROM v_holdings \
             WHERE account_id='acc-sp' AND instrument_id='inst-sp'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((qty - 80.0).abs() < 1e-6, "持仓 100 − 20 = 80，实际 {qty}");
    assert_eq!(cost_basis, 100_000, "批次总成本不变");

    assert_eq!(
        balance_snapshot(&conn),
        balances_before,
        "缩股无现金腿，余额不变"
    );
    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        0,
        "缩股本身不产生已实现盈亏"
    );

    let on = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-02-01").unwrap();
    assert!(
        (on - 80.0).abs() < 1e-6,
        "as-of 应含缩股腿：100 − 20 = 80，实际 {on}"
    );
    let before = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-01-31").unwrap();
    assert!(
        (before - 100.0).abs() < 1e-6,
        "调整日之前不含 Δ，实际 {before}"
    );
}

#[test]
fn split_reverse_tail_difference_lands_on_last_batch() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            200.0,
            20_000,
            "2026-01-15",
        ),
    )
    .unwrap();

    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", -30.0)).unwrap();

    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots.len(), 2);
    assert!((lots[0].1 - 90.0).abs() < 1e-9, "非末批次剩余 = 100 × 0.9");
    assert_eq!(lots[0].2, 11_111, "非末批次独立取整上升 round(10000 ÷ f)");
    assert!((lots[1].1 - 180.0).abs() < 1e-9, "末批次剩余 = 200 × 0.9");
    assert_eq!(
        lots[1].2, 22_222,
        "末批次闭合尾差：round((50000 − 90×11111÷100) × 100 ÷ 180)"
    );

    let canonical: i64 = conn
        .query_row(
            "SELECT CAST(COALESCE(SUM(t.amount_cents),0) - COALESCE((\
               SELECT SUM(CAST(ROUND(q * cpu / 100.0) AS INTEGER)) FROM (\
                 SELECT s.quantity AS q, s.cost_per_unit_cents AS cpu FROM security_lot_sales s \
                 JOIN security_lots l ON l.id = s.lot_id WHERE l.instrument_id='inst-sp' \
                 UNION ALL \
                 SELECT c.quantity, c.cost_per_unit_cents FROM security_lot_conversions c \
                 JOIN security_lots l ON l.id = c.lot_id WHERE l.instrument_id='inst-sp')),0) AS INTEGER) \
             FROM security_lots l JOIN transactions t ON t.id = l.buy_transaction_id \
             WHERE l.instrument_id='inst-sp' AND l.remaining_quantity > 0",
        [],
        |r| r.get(0),
    )
    .unwrap();
    assert_eq!(canonical, 50_000, "Σ 权威批次剩余成本精确不变");

    let lot_sum: f64 = lots.iter().map(|l| l.1).sum();
    let as_of = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-02-01").unwrap();
    let view_qty: f64 = conn
        .query_row(
            "SELECT quantity FROM v_holdings WHERE account_id='acc-sp' AND instrument_id='inst-sp'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((lot_sum - 270.0).abs() < 1e-9);
    assert!(
        (as_of - lot_sum).abs() < 1e-6,
        "as-of {as_of} 应对齐批次合计 {lot_sum}"
    );
    assert!(
        (view_qty - lot_sum).abs() < 1e-6,
        "持仓视图 {view_qty} 应对齐批次合计 {lot_sum}"
    );

    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 0);
}

#[test]
fn split_reverse_partial_sell_settles_at_restated_cost() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", -20.0)).unwrap();

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            60.0,
            150_000,
            "2026-03-01",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 15_000);

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            20.0,
            150_000,
            "2026-04-01",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 20_000);
}

#[test]
fn split_guards_return_coded_errors() {
    let conn = open();
    seed_split_scene(&conn);
    seed_account(&conn, "acc-cash", "现金户", "cash", "CNY", 0);
    seed_instrument(&conn, "inst-other", "600519", "贵州茅台", "CNY", "sh");

    let err = |r: ledger_infra::error::Result<ledger_transaction::TransactionWrite>| {
        r.expect_err("应被拒绝")
    };

    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-other", 10.0),
    ));
    assert_eq!(e.code().unwrap(), "trade.split-no-holding", "{e:?}");
    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-other", -10.0),
    ));
    assert_eq!(
        e.code().unwrap(),
        "trade.split-shrink-not-less-than-holding",
        "{e:?}"
    );

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();

    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-sp", 0.0),
    ));
    assert_eq!(e.code().unwrap(), "trade.split-quantity-zero");
    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-sp", -100.0),
    ));
    assert_eq!(
        e.code().unwrap(),
        "trade.split-shrink-not-less-than-holding",
        "{e:?}"
    );
    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-sp", -150.0),
    ));
    assert_eq!(
        e.code().unwrap(),
        "trade.split-shrink-not-less-than-holding",
        "{e:?}"
    );

    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-cash", "inst-sp", 10.0),
    ));
    assert_eq!(e.code().unwrap(), "trade.split-account-not-investment");

    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.amount_cents = 100;
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-amount-forbidden");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.price_cents = Some(10_000);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-price-forbidden");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.fee_cents = Some(1);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-fee-forbidden");

    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.to_instrument_id = Some("inst-other".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-to-instrument-forbidden");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.to_account_id = Some("acc-cash".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-to-account-forbidden");

    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.funding_account_id = Some("acc-cash".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.funding-unsupported");

    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.merchant_name = Some("京东".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.merchant-unsupported");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.category_id = Some("cat-x".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.category-unsupported");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.policy_id = Some("pol-x".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.policy-unsupported");

    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-missing", 10.0),
    ));
    assert_eq!(e.code().unwrap(), "trade.split-instrument-not-found");

    let splits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE action='split'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(splits, 0, "被拒的份额调整不应落库");
    assert!((lots_of(&conn, "inst-sp")[0].1 - 100.0).abs() < 1e-9);
}

#[test]
fn split_kind_change_is_rejected() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0))
        .unwrap()
        .id;

    let e = update_transaction_internal(
        &conn,
        &split_id,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            10.0,
            10_000,
            "2026-02-01",
        ),
    )
    .expect_err("改出 split 应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-kind-change-forbidden");
    let buy_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            10.0,
            10_000,
            "2026-01-20",
        ),
    )
    .unwrap()
    .id;
    let e = update_transaction_internal(&conn, &buy_id, make_split_input("acc-sp", "inst-sp", 5.0))
        .expect_err("改入 split 应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-kind-change-forbidden");
}

fn audit_rows(conn: &rusqlite::Connection, split_id: &str) -> Vec<(f64, f64, i64, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT a.remaining_quantity_before, a.remaining_quantity_after, \
             a.cost_per_unit_cents_before, a.cost_per_unit_cents_after \
             FROM security_lot_adjustments a JOIN security_lots l ON l.id = a.lot_id \
             WHERE a.transaction_id=?1 ORDER BY l.rowid",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![split_id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

fn split_extension(conn: &rusqlite::Connection, split_id: &str) -> (f64, i64) {
    conn.query_row(
        "SELECT st.quantity, t.amount_cents FROM security_transactions st \
         JOIN transactions t ON t.id = st.transaction_id WHERE st.transaction_id=?1",
        rusqlite::params![split_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

#[test]
fn split_update_reallocates_lots_to_new_input() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0))
        .unwrap()
        .id;
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(150.0, 150.0, 66_667)]);

    let before = balance_snapshot(&conn);
    update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", 100.0),
    )
    .unwrap();

    assert_eq!(lots_of(&conn, "inst-sp"), vec![(200.0, 200.0, 50_000)]);
    assert_eq!(
        audit_rows(&conn, &split_id),
        vec![(100.0, 200.0, 100_000, 50_000)]
    );
    assert_eq!(split_extension(&conn, &split_id), (100.0, 0));
    assert_eq!(before, balance_snapshot(&conn), "修改前后余额不变");
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 0, "split 恒零已实现盈亏");
}

#[test]
fn split_update_crosses_growth_and_shrink_direction() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(200.0, 200.0, 50_000)]);

    update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", -40.0),
    )
    .unwrap();
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(60.0, 60.0, 166_667)]);

    update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", 300.0),
    )
    .unwrap();
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(400.0, 400.0, 25_000)]);
}

#[test]
fn split_update_leaves_later_buys_untouched() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0))
        .unwrap()
        .id;
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            200.0,
            20_000,
            "2026-03-01",
        ),
    )
    .unwrap();
    let later_buy = lots_of(&conn, "inst-sp")[1];

    update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", 100.0),
    )
    .unwrap();
    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots[0], (200.0, 200.0, 5_000));
    assert_eq!(
        lots[1], later_buy,
        "后续买入的批次不因历史 split 的修改而重述"
    );
    assert_eq!(
        audit_rows(&conn, &split_id).len(),
        1,
        "审计只覆盖在场批次 A"
    );

    delete_transaction_internal(&conn, &split_id).unwrap();
    assert_eq!(
        lots_of(&conn, "inst-sp"),
        vec![(100.0, 100.0, 10_000), later_buy]
    );
}

#[test]
fn split_delete_restores_lots_exactly_with_rounding() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            200.0,
            20_000,
            "2026-01-15",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            20_000,
            "2026-01-20",
        ),
    )
    .unwrap();
    let lots_before_split = lots_of(&conn, "inst-sp");
    let pnl_before_split = realized_pnl_of(&conn, "inst-sp");
    let balances_before_split = balance_snapshot(&conn);

    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 30.0))
        .unwrap()
        .id;
    assert_ne!(
        lots_of(&conn, "inst-sp"),
        lots_before_split,
        "重述应改变批次"
    );
    assert_eq!(
        audit_rows(&conn, &split_id).len(),
        2,
        "每个在用批次一行审计"
    );

    delete_transaction_internal(&conn, &split_id).unwrap();

    assert_eq!(
        lots_of(&conn, "inst-sp"),
        lots_before_split,
        "删除后批次逐值精确还原到调整前（含舍入）"
    );
    assert!(audit_rows(&conn, &split_id).is_empty(), "审计随删除消失");
    let ext: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
            rusqlite::params![split_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ext, 0, "split 扩展行随删除消失");
    let deleted: i64 = conn
        .query_row(
            "SELECT is_deleted FROM transactions WHERE id=?1",
            rusqlite::params![split_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted, 1, "交易行软删");
    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        pnl_before_split,
        "先序卖出的已实现盈亏不受删除影响"
    );
    assert_eq!(
        balance_snapshot(&conn),
        balances_before_split,
        "删除前后余额不变"
    );
}

#[test]
fn split_downstream_consumption_guards_update_and_delete() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    let sell_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            50.0,
            100_000,
            "2026-03-01",
        ),
    )
    .unwrap()
    .id;
    let lots_after_sell = lots_of(&conn, "inst-sp");

    let e = update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", 60.0),
    )
    .expect_err("被下游消耗的份额调整修改应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-update", "{e:?}");
    let e = delete_transaction_internal(&conn, &split_id)
        .expect_err("被下游消耗的份额调整删除应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-delete", "{e:?}");
    assert_eq!(lots_of(&conn, "inst-sp"), lots_after_sell);
    assert_eq!(audit_rows(&conn, &split_id).len(), 1);
    assert_eq!(split_extension(&conn, &split_id), (100.0, 0));

    delete_transaction_internal(&conn, &sell_id).unwrap();
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(200.0, 200.0, 50_000)]);
    delete_transaction_internal(&conn, &split_id).unwrap();
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(100.0, 100.0, 100_000)]);
}

#[test]
fn split_downstream_convert_guards_delete() {
    let conn = open();
    seed_split_scene(&conn);
    seed_instrument(&conn, "inst-other", "600519", "贵州茅台", "CNY", "sh");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    create_transaction_internal(
        &conn,
        make_convert_input("acc-sp", "inst-sp", "inst-other", 80.0, 80.0, 800, 800, 0),
    )
    .unwrap();

    let e = update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", 60.0),
    )
    .expect_err("被下游转换消耗的份额调整修改应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-update", "{e:?}");
    let e = delete_transaction_internal(&conn, &split_id)
        .expect_err("被下游转换消耗的份额调整删除应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-delete", "{e:?}");
}

#[test]
fn split_later_split_guards_earlier_split_delete() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let first = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    let second = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(300.0, 300.0, 33_333)]);

    let e = delete_transaction_internal(&conn, &first).expect_err("前一行被后一行重述应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-delete", "{e:?}");
    let e = update_transaction_internal(&conn, &first, make_split_input("acc-sp", "inst-sp", 50.0))
        .expect_err("前一行被后一行重述应拒绝修改");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-update", "{e:?}");

    delete_transaction_internal(&conn, &second).unwrap();
    assert_eq!(
        lots_of(&conn, "inst-sp"),
        vec![(200.0, 200.0, 50_000)],
        "删后一行先还原到它重述前（= 前一行重述后）"
    );
    delete_transaction_internal(&conn, &first).unwrap();
    assert_eq!(
        lots_of(&conn, "inst-sp"),
        vec![(100.0, 100.0, 100_000)],
        "链条自后向前拆除后精确还原到调整前"
    );
}

#[test]
fn split_upstream_consumption_does_not_block_delete() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let sell_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            100_000,
            "2026-01-20",
        ),
    )
    .unwrap()
    .id;
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 60.0))
        .unwrap()
        .id;
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(200.0, 120.0, 50_000)]);

    delete_transaction_internal(&conn, &split_id).unwrap();
    assert_eq!(
        lots_of(&conn, "inst-sp"),
        vec![(100.0, 60.0, 100_000)],
        "回补到 split 前（先序卖出后的）批次状态"
    );
    let sell_matches: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lot_sales WHERE sell_transaction_id=?1",
            rusqlite::params![sell_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sell_matches, 1, "先序卖出的匹配记录完整保留");
}

#[test]
fn buy_restated_by_active_split_cannot_update_or_delete() {
    let conn = open();
    seed_split_scene(&conn);
    let buy_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap()
    .id;
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0)).unwrap();

    let e = update_transaction_internal(
        &conn,
        &buy_id,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            110.0,
            10_000,
            "2026-01-10",
        ),
    )
    .expect_err("被重述批次的买入修改应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.consumed-by-split-update");
    let e = delete_transaction_internal(&conn, &buy_id).expect_err("被重述批次的买入删除应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.consumed-by-split-delete");
}

#[test]
fn split_keeps_all_account_balances_unchanged() {
    let conn = open();
    seed_split_scene(&conn);
    seed_account(&conn, "acc-hole", "黑洞", "other", "CNY", 0);
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hole'", [])
        .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();

    let before = balance_snapshot(&conn);
    let split_id =
        create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 33.976))
            .unwrap()
            .id;
    let after = balance_snapshot(&conn);
    assert_eq!(
        before, after,
        "split 落账前后全部账户余额（含黑洞）完全不变"
    );

    let (amount, native, currency): (i64, i64, String) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents, currency_code FROM transactions WHERE id=?1",
            rusqlite::params![split_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((amount, native), (0, 0));
    assert_eq!(currency, "CNY");
}

#[test]
fn portfolio_trend_includes_split_leg() {
    let conn = open();
    seed_split_scene(&conn);
    tauri_app_lib::test_support::seed_price_history(
        &conn,
        "ph-sp",
        "inst-sp",
        "2026-02-02",
        10_000,
        "CNY",
    );
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0)).unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.points.len(), 1);
    assert_eq!(
        trend.points[0].market_value_cents, 15_000,
        "周点市值 = (100 + 50) × 1.00 元 = 15000 分"
    );
}

#[test]
fn portfolio_trend_includes_reverse_split_leg() {
    let conn = open();
    seed_split_scene(&conn);
    tauri_app_lib::test_support::seed_price_history(
        &conn,
        "ph-rs",
        "inst-sp",
        "2026-02-02",
        10_000,
        "CNY",
    );
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", -40.0)).unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.points.len(), 1);
    assert_eq!(
        trend.points[0].market_value_cents, 6_000,
        "周点市值 = (100 − 40) × 1.00 元 = 6000 分"
    );
}

#[test]
fn get_transaction_split_returns_signed_delta() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();

    let plus_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 10.0))
        .unwrap()
        .id;
    let plus = trade::get_transaction_split(&conn, &plus_id).unwrap();
    assert_eq!(plus.instrument_id, "inst-sp");
    assert_eq!(plus.symbol, "502010");
    assert_eq!(plus.instrument_name.as_deref(), Some("证券基金"));
    assert!((plus.quantity - 10.0).abs() < 1e-9, "正向 Δ 原样回填");

    let minus_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", -20.0))
        .unwrap()
        .id;
    let minus = trade::get_transaction_split(&conn, &minus_id).unwrap();
    assert!((minus.quantity + 20.0).abs() < 1e-9, "负向 Δ 保持符号");

    let err = trade::get_transaction_split(&conn, "no-such-txn").unwrap_err();
    assert!(
        matches!(&err, AppError::Coded { code, .. } if code == "trade.split-detail-not-found"),
        "非 split 交易应得码化 NotFound，got: {err:?}"
    );
}

#[test]
fn split_zero_leg_skips_fx_lookup_on_foreign_currency_account() {
    let conn = open();
    seed_account(&conn, "acc-sp-usd", "美股户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-sp-usd", "TSLA", "特斯拉", "USD", "unknown");
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.2, &["2026-01-10"]);

    create_transaction_internal(
        &conn,
        make_buy_input("acc-sp-usd", "inst-sp-usd", 10.0, 1_000_000, 0),
    )
    .unwrap();
    let split_id =
        create_transaction_internal(&conn, make_split_input("acc-sp-usd", "inst-sp-usd", 5.0))
            .unwrap()
            .id;

    let (amount_cents, amount_native_cents, currency, fx_rate_used, fx_rate_source): (
        i64,
        i64,
        String,
        Option<f64>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents, currency_code, fx_rate_used, fx_rate_source \
             FROM transactions WHERE id=?1",
            rusqlite::params![split_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 0, "无现金腿行金额恒 0");
    assert_eq!(
        amount_native_cents, 0,
        "零腿不经折算入口：外币行本位币仍恒 0——改走折算入口即 fx.rate-missing 红"
    );
    assert_eq!(currency, "USD", "行币种 = 账户币种");
    assert_eq!(fx_rate_used, None, "无折算无留痕（#1548 空值语义）");
    assert_eq!(fx_rate_source, None, "无折算无来源");
}
