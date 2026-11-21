use ledger_transaction::TransactionInput;
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

fn seed_convert_scene(conn: &rusqlite::Connection) {
    seed_account(conn, "acc-cv", "基金户", "investment", "CNY", 0);
    seed_instrument(conn, "inst-out", "006793", "转出基金", "CNY", "unknown");
    seed_instrument(conn, "inst-in", "519700", "转入基金", "CNY", "unknown");
    seed_account(conn, "acc-hole", "黑洞", "other", "CNY", 0);
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hole'", [])
        .unwrap();
    ledger_accounts::balance::refresh_account_balances(conn, &["acc-hole"]).unwrap();
}

fn fund_lot(conn: &rusqlite::Connection, instrument_id: &str) -> Option<(f64, f64, i64)> {
    conn.query_row(
        "SELECT initial_quantity, remaining_quantity, cost_per_unit_cents \
         FROM security_lots WHERE instrument_id=?1",
        rusqlite::params![instrument_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

#[test]
fn convert_moves_cost_basis_and_keeps_zero_pnl_and_balances() {
    let conn = open();
    seed_convert_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    assert!(
        (holdings::holdings_as_of(&conn, Some("inst-out"), "2026-01-15").unwrap() - 10.0).abs()
            < 1e-9
    );

    let before = balance_snapshot(&conn);
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;

    let (kind, amount_cents, amount_native_cents, currency): (TransactionKind, i64, i64, String) =
        conn.query_row(
            "SELECT kind, amount_cents, amount_native_cents, currency_code FROM transactions WHERE id=?1",
            rusqlite::params![convert_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(kind, TransactionKind::Convert);
    assert_eq!(amount_cents, 1_000, "行金额锚点 = FIFO 结转成本");
    assert_eq!(amount_native_cents, 1_000, "本位币折算 1:1（CNY）");
    assert_eq!(currency, "CNY");

    let (action, quantity, price_cents, fee, to_instrument, to_quantity, out_amount, in_amount): (
        String,
        f64,
        i64,
        i64,
        String,
        f64,
        i64,
        i64,
    ) = conn
        .query_row(
            "SELECT action, quantity, price_cents, fee_cents, to_instrument_id, to_quantity, \
             out_amount_cents, in_amount_cents FROM security_transactions WHERE transaction_id=?1",
            rusqlite::params![convert_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(action, "convert");
    assert!((quantity - 10.0).abs() < 1e-9);
    assert_eq!(price_cents, 11_000, "转出腿反算单价 1.10 元");
    assert_eq!(fee, 0);
    assert_eq!(to_instrument, "inst-in");
    assert!((to_quantity - 10.0).abs() < 1e-9);
    assert_eq!(out_amount, 1_100, "转出金额存确认单权威（展示口径）");
    assert_eq!(in_amount, 1_100);

    let (lot_id, consumed_qty, consumed_cpu, consumed_cost): (String, f64, i64, i64) = conn
        .query_row(
            "SELECT lot_id, quantity, cost_per_unit_cents, cost_cents \
             FROM security_lot_conversions WHERE transaction_id=?1",
            rusqlite::params![convert_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert!((consumed_qty - 10.0).abs() < 1e-9);
    assert_eq!(consumed_cpu, 10_000);
    assert_eq!(consumed_cost, 1_000, "单批结转成本精确到分");

    let out_lot = fund_lot(&conn, "inst-out").unwrap();
    assert!((out_lot.1 - 0.0).abs() < 1e-9, "转出腿耗尽原批次");
    let in_lot = fund_lot(&conn, "inst-in").unwrap();
    assert!((in_lot.0 - 10.0).abs() < 1e-9);
    assert!((in_lot.1 - 10.0).abs() < 1e-9);
    assert_eq!(in_lot.2, 10_000, "转入批次每份成本 = 结转成本 ÷ 转入份额");

    let anchor: String = conn
        .query_row(
            "SELECT l.id FROM security_lots l JOIN security_lot_conversions c ON c.lot_id = l.id \
             WHERE c.transaction_id=?1",
            rusqlite::params![convert_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(anchor, lot_id);

    let sales: i64 = conn
        .query_row("SELECT COUNT(*) FROM security_lot_sales", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sales, 0, "转换零已实现盈亏（不写匹配行）");

    assert_eq!(
        before,
        balance_snapshot(&conn),
        "转换落账前后全部账户余额不变"
    );
    tauri_app_lib::test_support::assert_balance_cache_matches_realtime(&conn);

    let out_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id='inst-out'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(out_rows, 0, "转出标的清仓");
    let in_cost: i64 = conn
        .query_row(
            "SELECT cost_basis_cents FROM v_holdings WHERE instrument_id='inst-in'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        in_cost, 1_000,
        "转入持仓成本 = 结转成本（不按转入日市值重置）"
    );

    assert!(
        (holdings::holdings_as_of(&conn, Some("inst-out"), "2026-02-05").unwrap() - 0.0).abs()
            < 1e-9
    );
    assert!(
        (holdings::holdings_as_of(&conn, Some("inst-in"), "2026-02-05").unwrap() - 10.0).abs()
            < 1e-9
    );
    assert!(
        (holdings::holdings_as_of(&conn, Some("inst-out"), "2026-01-15").unwrap() - 10.0).abs()
            < 1e-9
    );
    let portfolio_before = holdings::holdings_as_of(&conn, None, "2026-01-15").unwrap();
    let portfolio_after = holdings::holdings_as_of(&conn, None, "2026-02-05").unwrap();
    assert!(
        (portfolio_before - portfolio_after).abs() < 1e-9,
        "转换前后组合总份额不变（{portfolio_before} vs {portfolio_after}）"
    );
}

#[test]
fn convert_carried_cost_closes_on_exhaustion_with_rounding() {
    let conn = open();
    seed_convert_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            3.0,
            12_345,
            "2026-01-10",
        ),
    )
    .unwrap();
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 3.0, 2.5, 400, 400, 0),
    )
    .unwrap()
    .id;
    let carried: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            rusqlite::params![convert_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(carried, 370, "转换批次耗尽时闭合到结转成本（精确到分）");
    assert_eq!(fund_lot(&conn, "inst-in").unwrap().2, 14_800);

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-cv",
            "inst-in",
            2.5,
            20_000,
            "2026-02-10",
        ),
    )
    .unwrap();
    let realized: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(realized_pnl_cents),0) FROM security_lot_sales",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(realized, 130, "盈亏 = 卖出 500 − 结转成本 370");
    assert_eq!(
        realized,
        500 - 370,
        "闭合不变式：Σ 已实现盈亏 = Σ 卖出金额 − Σ 买入金额"
    );
}

#[test]
fn convert_partial_then_sell_out_lot_closes_with_prior_convert() {
    let conn = open();
    seed_convert_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 4.0, 4.0, 440, 440, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-cv",
            "inst-out",
            6.0,
            12_000,
            "2026-02-10",
        ),
    )
    .unwrap();
    let sell_cost: (i64, i64) = conn
        .query_row(
            "SELECT t.amount_cents, SUM(sls.realized_pnl_cents) \
             FROM transactions t JOIN security_lot_sales sls ON sls.sell_transaction_id=t.id \
             WHERE t.kind='sell' GROUP BY t.id",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(sell_cost.0, 720);
    assert_eq!(sell_cost.1, 120, "卖出盈亏 = 720 − (1000 − 400)");

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-cv",
            "inst-in",
            4.0,
            13_000,
            "2026-03-10",
        ),
    )
    .unwrap();
    let realized: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(realized_pnl_cents),0) FROM security_lot_sales",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(realized, 240);
    assert_eq!(realized, (720 + 520) - 1_000, "Σ 盈亏 = Σ 卖出 − Σ 买入");
}

struct GuardRow {
    name: &'static str,
    expected_code: &'static str,
    expected: Option<&'static str>,
    action: fn(&rusqlite::Connection) -> Result<(), AppError>,
}

#[test]
fn convert_guard_rejection_table() {
    use rusqlite::Connection;

    fn scene(conn: &Connection) {
        seed_convert_scene(conn);
        create_transaction_internal(
            conn,
            make_trade_input(
                TransactionKind::Buy,
                "acc-cv",
                "inst-out",
                10.0,
                10_000,
                "2026-01-10",
            ),
        )
        .unwrap();
    }

    fn with_convert(
        conn: &Connection,
        mutate: impl FnOnce(&mut TransactionInput),
    ) -> Result<(), AppError> {
        scene(conn);
        let mut input =
            make_convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0);
        mutate(&mut input);
        create_transaction_internal(conn, input).map(|_| ())
    }

    let rows: &[GuardRow] = &[
        GuardRow {
            name: "缺转出标的",
            expected_code: "trade.convert-instrument-required",
            expected: Some("转换必须指定转出标的"),
            action: |conn| with_convert(conn, |i| i.instrument_id = None),
        },
        GuardRow {
            name: "缺转入标的",
            expected_code: "trade.convert-to-instrument-required",
            expected: Some("转换必须指定转入标的"),
            action: |conn| with_convert(conn, |i| i.to_instrument_id = None),
        },
        GuardRow {
            name: "两标的相同",
            expected_code: "trade.convert-same-instrument",
            expected: None,
            action: |conn| {
                with_convert(conn, |i| {
                    i.to_instrument_id = Some("inst-out".into());
                })
            },
        },
        GuardRow {
            name: "转出标的不存在",
            expected_code: "trade.convert-instrument-not-found",
            expected: None,
            action: |conn| {
                with_convert(conn, |i| {
                    i.instrument_id = Some("inst-missing".into());
                })
            },
        },
        GuardRow {
            name: "转入标的不存在",
            expected_code: "trade.convert-to-instrument-not-found",
            expected: None,
            action: |conn| {
                with_convert(conn, |i| {
                    i.to_instrument_id = Some("inst-missing".into());
                })
            },
        },
        GuardRow {
            name: "转出份额非正",
            expected_code: "trade.convert-quantity-positive",
            expected: None,
            action: |conn| with_convert(conn, |i| i.quantity = Some(0.0)),
        },
        GuardRow {
            name: "转入份额非正",
            expected_code: "trade.convert-to-quantity-positive",
            expected: None,
            action: |conn| with_convert(conn, |i| i.to_quantity = Some(0.0)),
        },
        GuardRow {
            name: "转出金额非正",
            expected_code: "trade.convert-out-amount-positive",
            expected: None,
            action: |conn| with_convert(conn, |i| i.out_amount_cents = Some(0)),
        },
        GuardRow {
            name: "转入金额非正",
            expected_code: "trade.convert-in-amount-positive",
            expected: None,
            action: |conn| with_convert(conn, |i| i.in_amount_cents = Some(0)),
        },
        GuardRow {
            name: "非投资账户",
            expected_code: "trade.convert-account-not-investment",
            expected: None,
            action: |conn| {
                scene(conn);
                seed_account(conn, "acc-cash-cv", "现金", "cash", "CNY", 0);
                let input =
                    make_convert_input("acc-cash-cv", "inst-out", "inst-in", 1.0, 1.0, 100, 100, 0);
                create_transaction_internal(conn, input).map(|_| ())
            },
        },
        GuardRow {
            name: "携带转入账户（跨账户）",
            expected_code: "trade.convert-to-account-forbidden",
            expected: None,
            action: |conn| with_convert(conn, |i| i.to_account_id = Some("acc-else".into())),
        },
        GuardRow {
            name: "携带出资账户",
            expected_code: "transaction.funding-unsupported",
            expected: Some("不能携带出资账户"),
            action: |conn| with_convert(conn, |i| i.funding_account_id = Some("acc-fund".into())),
        },
        GuardRow {
            name: "转出超持仓",
            expected_code: "trade.insufficient-holding",
            expected: None,
            action: |conn| with_convert(conn, |i| i.quantity = Some(11.0)),
        },
        GuardRow {
            name: "携带商户",
            expected_code: "transaction.merchant-unsupported",
            expected: Some("不能携带商户"),
            action: |conn| with_convert(conn, |i| i.merchant_name = Some("某商户".into())),
        },
        GuardRow {
            name: "携带分类",
            expected_code: "transaction.category-unsupported",
            expected: Some("不能携带分类"),
            action: |conn| with_convert(conn, |i| i.category_id = Some("cat-1".into())),
        },
        GuardRow {
            name: "携带保单",
            expected_code: "transaction.policy-unsupported",
            expected: Some("不能挂保单"),
            action: |conn| with_convert(conn, |i| i.policy_id = Some("pol-1".into())),
        },
    ];

    for row in rows {
        let conn = open();
        let err = match (row.action)(&conn) {
            Err(err) => err,
            Ok(()) => panic!("守卫行「{}」失败：动作意外成功，应被拒绝", row.name),
        };
        match err {
            AppError::Coded { code, message, .. } => {
                assert_eq!(
                    code, row.expected_code,
                    "守卫行「{}」失败：错误码不符",
                    row.name
                );
                if let Some(expected) = row.expected {
                    assert!(
                        message.contains(expected),
                        "守卫行「{}」失败：期望文案「{}」未出现，got: {message}",
                        row.name,
                        expected
                    );
                }
            }
            other => panic!(
                "守卫行「{}」失败：应返回 Coded（400），got: {other:?}",
                row.name
            ),
        }
        let conversions: i64 = conn
            .query_row("SELECT COUNT(*) FROM security_lot_conversions", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(conversions, 0, "守卫行「{}」不应落消耗记录", row.name);
    }
}

#[test]
fn convert_consumes_out_lots_in_fifo_across_batches() {
    let conn = open();
    seed_convert_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            3.0,
            12_345,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            4.0,
            5_000,
            "2026-01-11",
        ),
    )
    .unwrap();

    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 5.0, 5.0, 600, 600, 0),
    )
    .unwrap()
    .id;

    let mut stmt = conn
        .prepare(
            "SELECT c.quantity, c.cost_per_unit_cents, c.cost_cents FROM security_lot_conversions c \
             JOIN security_lots l ON l.id = c.lot_id WHERE c.transaction_id=?1 ORDER BY l.rowid ASC",
        )
        .unwrap();
    let consumed: Vec<(f64, i64, i64)> = stmt
        .query_map(rusqlite::params![convert_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    drop(stmt);
    assert_eq!(consumed.len(), 2, "跨两个批次消耗");
    assert!((consumed[0].0 - 3.0).abs() < 1e-9, "先耗尽先买批次");
    assert_eq!(consumed[0].1, 12_345);
    assert_eq!(consumed[0].2, 370, "耗尽批次闭合到买入锚点金额");
    assert!((consumed[1].0 - 2.0).abs() < 1e-9, "再吃下一批次");
    assert_eq!(consumed[1].1, 5_000);
    assert_eq!(consumed[1].2, 100, "非耗尽批次按份额 × 每份成本舍入");

    let carried: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            rusqlite::params![convert_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(carried, 470);
    let in_lot = fund_lot(&conn, "inst-in").unwrap();
    assert_eq!(in_lot.2, 9_400);
    assert!((in_lot.1 - 5.0).abs() < 1e-9);
    let out_remaining: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(remaining_quantity),0) FROM security_lots WHERE instrument_id='inst-out'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((out_remaining - 2.0).abs() < 1e-9);
    assert!(
        (holdings::holdings_as_of(&conn, Some("inst-out"), "2026-03-01").unwrap() - 2.0).abs()
            < 1e-9
    );
}

#[test]
fn convert_consuming_buy_lot_guards_buy_edit_and_delete() {
    let conn = open();
    seed_convert_scene(&conn);
    let buy_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap()
    .id;
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;

    let err = delete_transaction_internal(&conn, &buy_id).unwrap_err();
    assert!(
        matches!(&err, AppError::Coded { code, .. } if code == "trade.consumed-by-convert-delete"),
        "被转换消耗的买入删除应被拒绝，got: {err:?}"
    );
    let err = update_transaction_internal(
        &conn,
        &buy_id,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            5.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap_err();
    assert!(
        matches!(&err, AppError::Coded { code, .. } if code == "trade.consumed-by-convert-update"),
        "被转换消耗的买入修改应被拒绝，got: {err:?}"
    );
    assert!((fund_lot(&conn, "inst-out").unwrap().1 - 0.0).abs() < 1e-9);
    let conversions: i64 = conn
        .query_row("SELECT COUNT(*) FROM security_lot_conversions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(conversions, 1);
    assert!(fund_lot(&conn, "inst-in").is_some());

    delete_transaction_internal(&conn, &convert_id).unwrap();
    assert!((fund_lot(&conn, "inst-out").unwrap().1 - 10.0).abs() < 1e-9);
    assert!(fund_lot(&conn, "inst-in").is_none());
    delete_transaction_internal(&conn, &buy_id).unwrap();
    assert!(fund_lot(&conn, "inst-out").is_none());
}

#[test]
fn convert_delete_restores_out_lots_and_purges_in_lot() {
    let conn = open();
    seed_convert_scene(&conn);
    let buy_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap()
    .id;
    let after_buy = balance_snapshot(&conn);
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;

    delete_transaction_internal(&conn, &convert_id).unwrap();

    assert!((fund_lot(&conn, "inst-out").unwrap().1 - 10.0).abs() < 1e-9);
    assert!(fund_lot(&conn, "inst-in").is_none());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM security_lot_conversions", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE action='convert'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(after_buy, balance_snapshot(&conn));
    tauri_app_lib::test_support::assert_balance_cache_matches_realtime(&conn);
    delete_transaction_internal(&conn, &buy_id).unwrap();
    assert!(fund_lot(&conn, "inst-out").is_none());
}

#[test]
fn convert_delete_cascades_active_sell_on_in_lot() {
    let conn = open();
    seed_convert_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let after_buy = balance_snapshot(&conn);
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    let sell_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-cv",
            "inst-in",
            4.0,
            15_000,
            "2026-02-10",
        ),
    )
    .unwrap()
    .id;

    delete_transaction_internal(&conn, &convert_id).unwrap();

    let sell_deleted: i64 = conn
        .query_row(
            "SELECT is_deleted FROM transactions WHERE id=?1",
            rusqlite::params![sell_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sell_deleted, 1, "在用 sell 随转换删除级联软删");
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM security_lot_sales", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(fund_lot(&conn, "inst-in").is_none());
    assert!((fund_lot(&conn, "inst-out").unwrap().1 - 10.0).abs() < 1e-9);
    assert_eq!(after_buy, balance_snapshot(&conn));
    tauri_app_lib::test_support::assert_balance_cache_matches_realtime(&conn);
}

#[test]
fn convert_chain_guards_update_and_delete_front_leg() {
    let conn = open();
    seed_convert_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let first = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    let second = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-in", "inst-out", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;

    let err = update_transaction_internal(
        &conn,
        &first,
        make_convert_input("acc-cv", "inst-out", "inst-in", 5.0, 5.0, 550, 550, 0),
    )
    .unwrap_err();
    assert!(
        matches!(&err, AppError::Coded { code, .. } if code == "trade.consumed-by-convert-update"),
        "被后续转换消耗的前腿不可修改，got: {err:?}"
    );
    let err = delete_transaction_internal(&conn, &first).unwrap_err();
    assert!(
        matches!(&err, AppError::Coded { code, .. } if code == "trade.consumed-by-convert-delete"),
        "被后续转换消耗的前腿不可删除，got: {err:?}"
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM security_lot_conversions", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        2
    );

    delete_transaction_internal(&conn, &second).unwrap();
    delete_transaction_internal(&conn, &first).unwrap();
    assert!((fund_lot(&conn, "inst-out").unwrap().1 - 10.0).abs() < 1e-9);
    assert!(fund_lot(&conn, "inst-in").is_none());
}

#[test]
fn convert_update_guard_when_in_lot_partially_sold() {
    let conn = open();
    seed_convert_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-cv",
            "inst-in",
            4.0,
            15_000,
            "2026-02-10",
        ),
    )
    .unwrap();

    let err = update_transaction_internal(
        &conn,
        &convert_id,
        make_convert_input("acc-cv", "inst-out", "inst-in", 6.0, 6.0, 660, 660, 0),
    )
    .unwrap_err();
    assert!(
        matches!(&err, AppError::Coded { code, .. } if code == "trade.convert-partially-sold-update"),
        "转入批次已被后续卖出的转换不可修改，got: {err:?}"
    );
    match &err {
        AppError::Coded { message, .. } => assert!(
            message.contains("该转换的转入份额已被后续卖出"),
            "转换守卫文案主体应为转换而非买入，got: {message}"
        ),
        _ => panic!("应为 Coded 错误"),
    }
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM security_lot_sales", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!((fund_lot(&conn, "inst-in").unwrap().1 - 6.0).abs() < 1e-9);
}

#[test]
fn convert_update_reverts_across_batches_and_reapplies() {
    let conn = open();
    seed_convert_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            3.0,
            12_345,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            4.0,
            5_000,
            "2026-01-11",
        ),
    )
    .unwrap();
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 5.0, 5.0, 600, 600, 0),
    )
    .unwrap()
    .id;
    let before = balance_snapshot(&conn);
    assert!((fund_lot(&conn, "inst-in").unwrap().0 - 5.0).abs() < 1e-9);

    update_transaction_internal(
        &conn,
        &convert_id,
        make_convert_input("acc-cv", "inst-out", "inst-in", 2.0, 2.0, 240, 240, 0),
    )
    .unwrap();

    let in_lots: Vec<(f64, i64)> = {
        let mut stmt = conn
            .prepare("SELECT initial_quantity, cost_per_unit_cents FROM security_lots WHERE instrument_id='inst-in'")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(in_lots.len(), 1, "旧转入批次被清理、只留新批次");
    assert!((in_lots[0].0 - 2.0).abs() < 1e-9);
    assert_eq!(in_lots[0].1, 12_350);
    let remaining: Vec<(String, f64)> = {
        let mut stmt = conn
            .prepare("SELECT instrument_id, remaining_quantity FROM security_lots WHERE instrument_id='inst-out' AND remaining_quantity > 0 ORDER BY rowid")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(remaining.len(), 2, "两批转出标的各有剩余");
    assert!((remaining[0].1 - 1.0).abs() < 1e-9, "首批剩 1 份");
    assert!((remaining[1].1 - 4.0).abs() < 1e-9, "次批 4 份未被消耗");
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM security_lot_conversions", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        1
    );
    assert_eq!(before, balance_snapshot(&conn), "修改前后余额不变");
}

#[test]
fn convert_put_kind_change_rejected() {
    let conn = open();
    seed_convert_scene(&conn);
    let buy_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap()
    .id;
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;

    let err = update_transaction_internal(
        &conn,
        &buy_id,
        make_convert_input("acc-cv", "inst-out", "inst-in", 1.0, 1.0, 100, 100, 0),
    )
    .unwrap_err();
    assert!(
        matches!(&err, AppError::Coded { code, .. } if code == "trade.convert-kind-change-forbidden"),
        "改为 convert 应被拒绝，got: {err:?}"
    );
    let err = update_transaction_internal(
        &conn,
        &convert_id,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            1.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap_err();
    assert!(
        matches!(&err, AppError::Coded { code, .. } if code == "trade.convert-kind-change-forbidden"),
        "改出 convert 应被拒绝，got: {err:?}"
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE action='convert'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn get_transaction_convert_returns_two_legs() {
    let conn = open();
    seed_convert_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-out",
            10.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 900, 5),
    )
    .unwrap()
    .id;

    let detail = trade::get_transaction_convert(&conn, &convert_id).unwrap();
    assert_eq!(detail.out_instrument_id, "inst-out");
    assert_eq!(detail.out_symbol, "006793");
    assert!((detail.out_quantity - 10.0).abs() < 1e-9);
    assert_eq!(detail.out_amount_cents, 1_100);
    assert_eq!(detail.in_instrument_id, "inst-in");
    assert_eq!(detail.in_symbol, "519700");
    assert!((detail.in_quantity - 10.0).abs() < 1e-9);
    assert_eq!(detail.in_amount_cents, 900);
    assert_eq!(detail.fee_cents, 5);
    assert_eq!(detail.carried_cost_cents, 1_000, "结转成本 = 行金额锚点");
    assert_eq!(detail.currency_code, "CNY");

    let err = trade::get_transaction_convert(&conn, "no-such-txn").unwrap_err();
    assert!(
        matches!(&err, AppError::Coded { code, .. } if code == "trade.convert-detail-not-found"),
        "非转换交易应得码化 NotFound，got: {err:?}"
    );
}

#[test]
fn convert_native_cents_converted_via_trade_date_history() {
    let conn = open();
    seed_account(&conn, "acc-cv-usd", "美股", "investment", "USD", 0);
    seed_instrument(
        &conn,
        "inst-cv-usd-out",
        "VWRA",
        "先锋全市场",
        "USD",
        "unknown",
    );
    seed_instrument(&conn, "inst-cv-usd-in", "IUSA", "标普500", "USD", "unknown");
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.2, &["2026-01-10", "2026-02-01"]);

    create_transaction_internal(
        &conn,
        make_buy_input("acc-cv-usd", "inst-cv-usd-out", 10.0, 1_000_000, 0),
    )
    .unwrap();
    let convert_id = create_transaction_internal(
        &conn,
        make_convert_input(
            "acc-cv-usd",
            "inst-cv-usd-out",
            "inst-cv-usd-in",
            10.0,
            10.0,
            1_100,
            1_100,
            0,
        ),
    )
    .unwrap()
    .id;

    let (amount_cents, amount_native_cents, fx_rate_used, fx_rate_source): (
        i64,
        i64,
        Option<f64>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents, fx_rate_used, fx_rate_source \
             FROM transactions WHERE id=?1",
            rusqlite::params![convert_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        amount_cents, 100_000,
        "行金额锚点 = FIFO 结转成本（10 × 100 元）"
    );
    assert_eq!(
        amount_native_cents, 720_000,
        "本位币折算按交易日历史（100000 × 7.2）——改回当期入口本断言即红"
    );
    assert_eq!(fx_rate_used, Some(7.2), "折算留痕随行");
    assert_eq!(fx_rate_source.as_deref(), Some("series"), "来源 = 序列命中");
}
