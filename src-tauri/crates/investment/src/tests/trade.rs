use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;
use rusqlite::{Connection, params};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{
    open, seed_account, seed_fx_history_weeks, seed_instrument, seed_market_price,
};

#[test]
fn price_scale_invariant_unit_price_is_ten_thousandths_of_yuan() {
    let conn = open();
    seed_account(&conn, "acc-fund", "基金户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-fund-abc",
        "000123",
        "某公募基金",
        "CNY",
        "unknown",
    );

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-fund", "inst-fund-abc", 3.0, 12345, 100),
    )
    .unwrap()
    .id;

    let (amount_cents, price_cents): (i64, i64) = conn
        .query_row(
            "SELECT t.amount_cents, st.price_cents FROM transactions t \
             JOIN security_transactions st ON st.transaction_id = t.id WHERE t.id=?1",
            params![buy_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        price_cents, 12345,
        "成交单价无损保留 4 位小数净值（1.2345 元）"
    );
    assert_eq!(
        amount_cents, 470,
        "行金额 = 3 × 1.2345 + 1 = 4.70 元 = 470 分"
    );

    let cost_per_unit: i64 = conn
        .query_row(
            "SELECT cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        cost_per_unit, 15678,
        "每份成本 = (3×12345 + 100×100)/3 = 15678 万分之一元"
    );

    let sell_id = create_transaction_internal(
        &conn,
        make_sell_input("acc-fund", "inst-fund-abc", 3.0, 13000, 0),
    )
    .unwrap()
    .id;
    let (sell_amount, realized_pnl): (i64, i64) = conn
        .query_row(
            "SELECT t.amount_cents, sls.realized_pnl_cents FROM transactions t \
             JOIN security_lot_sales sls ON sls.sell_transaction_id = t.id WHERE t.id=?1",
            params![sell_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(sell_amount, 390, "卖出收入 = 3 × 1.30 = 3.90 元 = 390 分");
    assert_eq!(realized_pnl, 390 - 470, "已实现盈亏 = 收入 − 成本（均分）");
}

#[test]
fn price_scale_invariant_v_holdings_market_value() {
    let conn = open();
    seed_account(&conn, "acc-scale", "刻度户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-scale",
        "000456",
        "净值保真基金",
        "CNY",
        "unknown",
    );

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-scale", "inst-scale", 100.0, 1234, 0),
    )
    .unwrap()
    .id;

    seed_market_price(&conn, "inst-scale", 5678, "CNY");

    let (cost_basis, market_value): (i64, i64) = conn
        .query_row(
            "SELECT cost_basis_cents, market_value_cents FROM v_holdings WHERE instrument_id=?1",
            params!["inst-scale"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        cost_basis, 1234,
        "成本 = 100 × 1234 ÷ 100 = 1234 分 = 12.34 元"
    );
    assert_eq!(
        market_value, 5678,
        "市值 = 100 × 5678 ÷ 100 = 5678 分 = 56.78 元"
    );
    let _ = buy_id;
}

#[test]
fn buy_transaction_creates_lot() {
    let conn = open();
    seed_account(&conn, "acc-test-buy", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );
    seed_instrument(&conn, "inst-test-nvda", "NVDA", "NVIDIA", "USD", "unknown");

    let input = make_buy_input("acc-test-buy", "inst-test-nvda", 10.0, 1_000_000, 500);
    let txn_id = create_transaction_internal(&conn, input).unwrap().id;

    let (kind, amount_cents, currency_code, amount_native, category_id, refund_of_id): (
        String,
        i64,
        String,
        i64,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT kind, amount_cents, currency_code, amount_native_cents, category_id, \
             refund_of_transaction_id FROM transactions WHERE id=?1",
            params![txn_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(kind, "buy");
    assert_eq!(amount_cents, 100500);
    assert_eq!(currency_code, "USD");
    assert_eq!(amount_native, amount_cents, "买入本位币与原始币种应 1:1");
    assert_eq!(category_id, None);
    assert_eq!(refund_of_id, None);

    let (action, quantity, price_cents, fee_cents): (String, f64, i64, i64) = conn
        .query_row(
            "SELECT action, quantity, price_cents, fee_cents FROM security_transactions WHERE transaction_id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(action, "buy");
    assert!((quantity - 10.0).abs() < 0.0001);
    assert_eq!(
        price_cents, 1_000_000,
        "成交单价存万分之一元（100 元 = 1_000_000）"
    );
    assert_eq!(fee_cents, 500);

    let (remaining_quantity, cost_per_unit): (f64, i64) = conn
        .query_row(
            "SELECT remaining_quantity, cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((remaining_quantity - 10.0).abs() < 0.0001);
    assert_eq!(
        cost_per_unit, 1_005_000,
        "每份成本存万分之一元（含费用摊薄）"
    );

    let (holding_quantity, cost_basis): (f64, i64) = conn
        .query_row(
            "SELECT quantity, cost_basis_cents FROM v_holdings WHERE id=?1",
            params!["acc-test-buy-inst-test-nvda-USD"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((holding_quantity - 10.0).abs() < 0.0001);
    assert_eq!(cost_basis, 100500);
}

#[test]
fn buy_native_cents_converted_via_amount_seam() {
    let conn = open();
    seed_account(&conn, "acc-test-conv", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.2, &["2026-01-10"]);
    seed_instrument(&conn, "inst-test-conv", "NVDA", "NVIDIA", "USD", "unknown");

    let input = make_buy_input("acc-test-conv", "inst-test-conv", 10.0, 1_000_000, 500);
    let txn_id = create_transaction_internal(&conn, input).unwrap().id;

    let (amount_cents, amount_native_cents): (i64, i64) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents FROM transactions WHERE id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 100500, "原始币种金额 = 数量×单价+手续费");
    assert_eq!(
        amount_native_cents, 723600,
        "本位币金额应经 convert_to_native_current 折算（100500 × 7.2）"
    );
}

#[test]
fn buy_row_traces_fx_rate_used_and_series_source() {
    let conn = open();
    seed_account(&conn, "acc-test-trace", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.2, &["2026-01-10"]);
    seed_instrument(&conn, "inst-test-trace", "NVDA", "NVIDIA", "USD", "unknown");

    let input = make_buy_input("acc-test-trace", "inst-test-trace", 10.0, 1_000_000, 500);
    let txn_id = create_transaction_internal(&conn, input).unwrap().id;

    let (fx_rate_used, fx_rate_source): (Option<f64>, Option<String>) = conn
        .query_row(
            "SELECT fx_rate_used, fx_rate_source FROM transactions WHERE id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(fx_rate_used, Some(7.2), "留痕 = 本笔使用的序列汇率值");
    assert_eq!(fx_rate_source.as_deref(), Some("series"), "来源 = 序列命中");
}

#[test]
fn buy_row_with_explicit_fx_rate_traces_explicit_source() {
    let conn = open();
    seed_account(&conn, "acc-test-expl", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.2, &["2026-01-10"]);
    seed_instrument(&conn, "inst-test-expl", "NVDA", "NVIDIA", "USD", "unknown");

    let mut input = make_buy_input("acc-test-expl", "inst-test-expl", 10.0, 1_000_000, 500);
    input.fx_rate = Some(7.0);
    let txn_id = create_transaction_internal(&conn, input).unwrap().id;

    let (amount_native_cents, fx_rate_used, fx_rate_source): (i64, Option<f64>, Option<String>) =
        conn.query_row(
            "SELECT amount_native_cents, fx_rate_used, fx_rate_source FROM transactions WHERE id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        amount_native_cents, 703500,
        "本位币金额 = (数量×单价+手续费) × 显式汇率（100500 × 7.0）"
    );
    assert_eq!(fx_rate_used, Some(7.0), "留痕 = 显式给定值本身");
    assert_eq!(
        fx_rate_source.as_deref(),
        Some("explicit"),
        "来源标为显式（序列点被跳过）"
    );
}

#[test]
fn split_rejects_explicit_fx_rate() {
    let conn = open();
    seed_account(&conn, "acc-test-split-fx", "基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-test-split-fx",
        "F001",
        "Fund One",
        "CNY",
        "unknown",
    );

    let mut input = make_split_input("acc-test-split-fx", "inst-test-split-fx", 100.0);
    input.fx_rate = Some(1.0);
    let err = create_transaction_internal(&conn, input).unwrap_err();
    match err {
        AppError::Coded { code, message, .. } => {
            assert_eq!(code, "trade.split-fx-rate-forbidden");
            assert!(message.contains("显式汇率"), "实际: {message}");
        }
        other => panic!("应为码化错误，实际: {other}"),
    }
}

#[test]
fn buy_update_native_cents_converted_via_amount_seam() {
    use ledger_transaction::update_transaction_internal;
    let conn = open();
    seed_account(&conn, "acc-test-conv-upd", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.2, &["2026-01-10", "2026-02-01"]);
    seed_instrument(
        &conn,
        "inst-test-conv-upd",
        "NVDA",
        "NVIDIA",
        "USD",
        "unknown",
    );

    let txn_id = create_transaction_internal(
        &conn,
        make_buy_input(
            "acc-test-conv-upd",
            "inst-test-conv-upd",
            10.0,
            1_000_000,
            500,
        ),
    )
    .unwrap()
    .id;

    let mut edited = make_buy_input("acc-test-conv-upd", "inst-test-conv-upd", 5.0, 1_200_000, 0);
    edited.date = "2026-02-01".into();
    update_transaction_internal(&conn, &txn_id, edited).unwrap();

    let (amount_cents, amount_native_cents): (i64, i64) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents FROM transactions WHERE id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 60000, "修改后金额 = 5×12000");
    assert_eq!(
        amount_native_cents, 432000,
        "修改后本位币金额应经 convert_to_native_current 折算（60000 × 7.2）"
    );
}

#[test]
fn sell_native_cents_converted_via_amount_seam() {
    let conn = open();
    seed_account(&conn, "acc-test-sell-conv", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.2, &["2026-01-10", "2026-01-20"]);
    seed_instrument(
        &conn,
        "inst-test-sell-conv",
        "TSLA",
        "Tesla",
        "USD",
        "unknown",
    );

    create_transaction_internal(
        &conn,
        make_buy_input(
            "acc-test-sell-conv",
            "inst-test-sell-conv",
            10.0,
            1_000_000,
            0,
        ),
    )
    .unwrap();
    let sell_id = create_transaction_internal(
        &conn,
        make_sell_input(
            "acc-test-sell-conv",
            "inst-test-sell-conv",
            4.0,
            1_100_000,
            0,
        ),
    )
    .unwrap()
    .id;

    let (amount_cents, amount_native_cents): (i64, i64) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents FROM transactions WHERE id=?1",
            params![sell_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 44000, "卖出入账 = 数量×单价−手续费");
    assert_eq!(
        amount_native_cents, 316800,
        "本位币金额应经 convert_to_native_current 折算（44000 × 7.2）"
    );
}

#[test]
fn buy_transaction_requires_investment_account() {
    let conn = open();
    seed_account(&conn, "acc-test-cash", "现金", "cash", "CNY", 0);
    seed_instrument(&conn, "inst-test-cny", "600519", "茅台", "CNY", "unknown");

    let input = make_buy_input("acc-test-cash", "inst-test-cny", 1.0, 10000, 0);
    let err = create_transaction_internal(&conn, input).unwrap_err();
    assert!(
        err.is_code("trade.buy-account-not-investment"),
        "非投资账户买入应以既定码拒绝，got: {err:?}"
    );
}

struct RejectionRow {
    name: &'static str,
    expected_code: &'static str,
    action: fn(&Connection) -> Result<(), AppError>,
    post: Option<fn(&Connection)>,
}

#[test]
fn missing_instrument_rejection_reason_table() {
    use ledger_transaction::update_transaction_internal;

    let rows: &[RejectionRow] = &[
        RejectionRow {
            name: "buy_create 引用不存在标的",
            expected_code: "trade.buy-instrument-not-found",
            action: |conn| {
                seed_account(conn, "acc-test-missing", "美股", "investment", "USD", 0);
                seed_fx_history_weeks(
                    conn,
                    "USD",
                    "CNY",
                    1.0,
                    &["2026-01-10", "2026-01-20", "2026-02-01"],
                );
                let input = make_buy_input("acc-test-missing", "inst-not-exist", 10.0, 10000, 0);
                create_transaction_internal(conn, input).map(|_| ())
            },
            post: Some(|conn| {
                let txns: i64 = conn
                    .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(txns, 0, "被拒的买入不应落交易行");
                let lots: i64 = conn
                    .query_row("SELECT COUNT(*) FROM security_lots", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(lots, 0, "被拒的买入不应有持仓批次");
            }),
        },
        RejectionRow {
            name: "sell_create 引用不存在标的",
            expected_code: "trade.sell-instrument-not-found",
            action: |conn| {
                seed_account(conn, "acc-test-sell-miss", "美股", "investment", "USD", 0);
                seed_fx_history_weeks(
                    conn,
                    "USD",
                    "CNY",
                    1.0,
                    &["2026-01-10", "2026-01-20", "2026-02-01"],
                );
                let input = make_sell_input("acc-test-sell-miss", "inst-not-exist", 5.0, 12000, 0);
                create_transaction_internal(conn, input).map(|_| ())
            },
            post: None,
        },
        RejectionRow {
            name: "update_buy 改引不存在标的",
            expected_code: "trade.buy-instrument-not-found",
            action: |conn| {
                seed_account(conn, "acc-test-upd-miss", "美股", "investment", "USD", 0);
                seed_fx_history_weeks(
                    conn,
                    "USD",
                    "CNY",
                    1.0,
                    &["2026-01-10", "2026-01-20", "2026-02-01"],
                );
                seed_instrument(
                    conn,
                    "inst-test-upd-miss",
                    "NVDA",
                    "NVIDIA",
                    "USD",
                    "unknown",
                );
                let txn_id = create_transaction_internal(
                    conn,
                    make_buy_input(
                        "acc-test-upd-miss",
                        "inst-test-upd-miss",
                        10.0,
                        1_000_000,
                        0,
                    ),
                )
                .unwrap()
                .id;
                let edited =
                    make_buy_input("acc-test-upd-miss", "inst-not-exist", 5.0, 1_200_000, 0);
                update_transaction_internal(conn, &txn_id, edited).map(|_| ())
            },
            post: Some(|conn| {
                let amount_cents: i64 = conn
                    .query_row("SELECT amount_cents FROM transactions", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(amount_cents, 100000, "原交易金额不应被修改");
                let remaining: f64 = conn
                    .query_row("SELECT remaining_quantity FROM security_lots", [], |r| {
                        r.get(0)
                    })
                    .unwrap();
                assert!((remaining - 10.0).abs() < 1e-9, "原持仓批次不应被清理");
            }),
        },
    ];
    for row in rows {
        let conn = open();
        let err = match (row.action)(&conn) {
            Err(err) => err,
            Ok(()) => panic!("拒绝理由表行「{}」失败：动作意外成功，应被拒绝", row.name),
        };
        match err {
            AppError::Coded { code, .. } => {
                assert_eq!(
                    code, row.expected_code,
                    "拒绝理由表行「{}」失败：错误码不符",
                    row.name
                );
            }
            other => panic!(
                "拒绝理由表行「{}」失败：应返回 Coded（400），got: {other:?}",
                row.name
            ),
        }
        if let Some(post) = row.post {
            post(&conn);
        }
    }
}

#[test]
fn buy_sell_to_account_carry_rejected_with_coded_error() {
    let conn = open();
    seed_account(&conn, "acc-ta", "美股", "investment", "USD", 0);
    seed_instrument(&conn, "inst-ta", "NVDA", "NVIDIA", "USD", "unknown");
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );

    let mut input = make_buy_input("acc-ta", "inst-ta", 10.0, 1_000_000, 0);
    input.to_account_id = Some("acc-ta".into());
    let err = create_transaction_internal(&conn, input).expect_err("携带转入账户的买入应被拒绝");
    assert_eq!(err.code().unwrap(), "trade.buy-to-account-forbidden");

    let mut input = make_sell_input("acc-ta", "inst-ta", 5.0, 1_200_000, 0);
    input.to_account_id = Some("acc-ta".into());
    let err = create_transaction_internal(&conn, input).expect_err("携带转入账户的卖出应被拒绝");
    assert_eq!(err.code().unwrap(), "trade.sell-to-account-forbidden");

    use ledger_transaction::update_transaction_internal;
    let txn_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-ta", "inst-ta", 10.0, 1_000_000, 0),
    )
    .unwrap()
    .id;
    let mut edited = make_buy_input("acc-ta", "inst-ta", 5.0, 1_200_000, 0);
    edited.to_account_id = Some("acc-ta".into());
    let err = update_transaction_internal(&conn, &txn_id, edited)
        .expect_err("修改携带转入账户的买入应被拒绝");
    assert_eq!(err.code().unwrap(), "trade.buy-to-account-forbidden");
    let (amount, remaining): (i64, f64) = conn
        .query_row(
            "SELECT t.amount_cents, l.remaining_quantity FROM transactions t \
             JOIN security_lots l ON l.buy_transaction_id = t.id WHERE t.id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount, 100_000, "原交易金额不应被修改");
    assert!((remaining - 10.0).abs() < 1e-9, "原持仓批次不应被清理");
}

#[test]
fn sell_transaction_matches_multiple_lots_fifo() {
    let conn = open();
    seed_account(&conn, "acc-test-sell", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );
    seed_instrument(&conn, "inst-test-sell", "TSLA", "Tesla", "USD", "unknown");

    let lot1_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 10.0, 1_000_000, 0),
    )
    .unwrap()
    .id;
    let lot2_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 5.0, 1_200_000, 0),
    )
    .unwrap()
    .id;

    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-10T00:00:00Z' WHERE buy_transaction_id=?1",
        params![lot1_txn],
    )
    .unwrap();
    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-15T00:00:00Z' WHERE buy_transaction_id=?1",
        params![lot2_txn],
    )
    .unwrap();

    let sell_txn = create_transaction_internal(
        &conn,
        make_sell_input("acc-test-sell", "inst-test-sell", 12.0, 1_500_000, 600),
    )
    .unwrap()
    .id;

    let (kind, amount_cents): (TransactionKind, i64) = conn
        .query_row(
            "SELECT kind, amount_cents FROM transactions WHERE id=?1",
            params![sell_txn],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, TransactionKind::Sell);
    assert_eq!(amount_cents, 179400);

    let rem1: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![lot1_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem1 - 0.0).abs() < 0.0001);
    let rem2: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![lot2_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem2 - 3.0).abs() < 0.0001);

    let rows: Vec<(f64, i64, i64, String)> = conn
        .prepare(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
             FROM security_lot_sales WHERE sell_transaction_id=?1 ORDER BY quantity DESC",
        )
        .unwrap()
        .query_map(params![sell_txn], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(rows.len(), 2);
    assert!((rows[0].0 - 10.0).abs() < 0.0001);
    assert_eq!(
        rows[0].1, 1_000_000,
        "卖出时批次单位成本快照同为万分之一元刻度"
    );
    assert_eq!(rows[0].2, 49500);
    assert_eq!(rows[0].3, "USD");
    assert!((rows[1].0 - 2.0).abs() < 0.0001);
    assert_eq!(rows[1].1, 1_200_000);
    assert_eq!(rows[1].2, 5900);
    assert_eq!(rows[1].3, "USD");
}

#[test]
fn sell_transaction_rejects_oversell() {
    let conn = open();
    seed_account(&conn, "acc-test-oversell", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );
    seed_instrument(
        &conn,
        "inst-test-oversell",
        "MSFT",
        "Microsoft",
        "USD",
        "unknown",
    );

    create_transaction_internal(
        &conn,
        make_buy_input("acc-test-oversell", "inst-test-oversell", 5.0, 1_000_000, 0),
    )
    .unwrap();

    let sell = make_sell_input("acc-test-oversell", "inst-test-oversell", 6.0, 12000, 0);
    assert!(create_transaction_internal(&conn, sell).is_err());
}

#[test]
fn sell_transaction_pnl_deducts_fee() {
    let conn = open();
    seed_account(&conn, "acc-test-pnl", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );
    seed_instrument(&conn, "inst-test-pnl", "AAPL", "Apple", "USD", "unknown");

    let buy_txn = create_transaction_internal(
        &conn,
        make_buy_input("acc-test-pnl", "inst-test-pnl", 10.0, 1_000_000, 0),
    )
    .unwrap()
    .id;
    let sell_txn = create_transaction_internal(
        &conn,
        make_sell_input("acc-test-pnl", "inst-test-pnl", 5.0, 1_200_000, 200),
    )
    .unwrap()
    .id;

    let rem: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem - 5.0).abs() < 0.0001);

    let (qty, cost, pnl, ccy): (f64, i64, i64, String) = conn
        .query_row(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
             FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell_txn],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert!((qty - 5.0).abs() < 0.0001);
    assert_eq!(cost, 1_000_000);
    assert_eq!(pnl, 9800);
    assert_eq!(ccy, "USD");

    let amount_cents: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            params![sell_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount_cents, 5 * 12000 - 200);
}

#[test]
fn get_transaction_trade_returns_buy_detail_with_instrument_display() {
    let conn = open();
    seed_account(&conn, "acc-inv", "证券户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-t", "600519", "贵州茅台", "USD", "unknown");
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );
    let id = create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-t", 100.0, 150_000, 500),
    )
    .unwrap()
    .id;

    let trade = trade::get_transaction_trade(&conn, &id).unwrap();
    assert_eq!(trade.instrument_id, "inst-t");
    assert_eq!(trade.symbol, "600519");
    assert_eq!(trade.instrument_name.as_deref(), Some("贵州茅台"));
    assert_eq!(
        trade.instrument_type, "stock",
        "明细带出标的类型（issue #302 表单形态切换）"
    );
    assert!((trade.quantity - 100.0).abs() < 1e-9);
    assert_eq!(trade.price_cents, 150_000, "明细单价万分之一元刻度");
    assert_eq!(trade.fee_cents, Some(500));
}

#[test]
fn get_transaction_trade_rejects_missing_or_non_trade_transaction() {
    let conn = open();
    seed_account(&conn, "acc-cash", "现金", "cash", "CNY", 0);
    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: Some("acc-cash".into()),
            funding: Vec::new(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            funding_account_id: None,
            source_order_no: None,
            note: None,
            date: "2026-01-10".into(),
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
    let err = trade::get_transaction_trade(&conn, &expense_id).unwrap_err();
    assert!(err.to_string().contains("无买卖明细"), "实际: {err}");
    let err = trade::get_transaction_trade(&conn, "no-such-txn").unwrap_err();
    assert!(err.to_string().contains("无买卖明细"), "实际: {err}");
}

#[test]
fn sell_full_clearance_with_fifo_noise_is_allowed() {
    let conn = open();
    seed_account(&conn, "acc-noise", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );
    seed_instrument(
        &conn,
        "inst-noise-src",
        "006793",
        "源标的",
        "USD",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-noise-dst",
        "519700",
        "目标标的",
        "USD",
        "unknown",
    );

    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-noise", "inst-noise-src", 11511.99, 10_000, 0),
    )
    .unwrap()
    .id;
    create_transaction_internal(
        &conn,
        make_convert_input(
            "acc-noise",
            "inst-noise-src",
            "inst-noise-dst",
            2791.12,
            2791.12,
            279_112,
            279_112,
            0,
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_convert_input(
            "acc-noise",
            "inst-noise-src",
            "inst-noise-dst",
            684.76,
            684.76,
            68_476,
            68_476,
            0,
        ),
    )
    .unwrap();

    let remaining: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        remaining < 8036.11,
        "复刻位模式：FIFO 扣减后剩余应低于 8036.11（真实账本 8036.109999999999），got {remaining}"
    );

    create_transaction_internal(
        &conn,
        make_sell_input("acc-noise", "inst-noise-src", 8036.11, 10_000, 0),
    )
    .unwrap();

    let remaining_after: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining_after, 0.0, "全清仓后批次精确归零");
}

#[test]
fn sell_guard_rejects_oversell_beyond_tolerance() {
    let conn = open();
    seed_account(&conn, "acc-eps", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );
    seed_instrument(&conn, "inst-eps", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-eps", "inst-eps", 100.0, 10_000, 0),
    )
    .unwrap();

    let err = create_transaction_internal(
        &conn,
        make_sell_input("acc-eps", "inst-eps", 100.000002, 10_000, 0),
    )
    .unwrap_err();
    assert!(
        err.is_code("trade.insufficient-holding"),
        "超容差超卖应保持原码拒绝，got: {err:?}"
    );

    create_transaction_internal(
        &conn,
        make_sell_input("acc-eps", "inst-eps", 100.0000005, 10_000, 0),
    )
    .unwrap();
}

#[test]
fn sell_full_clearance_consumes_noise_over_lot_exactly() {
    let conn = open();
    seed_account(&conn, "acc-dust", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );
    seed_instrument(&conn, "inst-dust", "MSFT", "Microsoft", "USD", "unknown");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-dust", "inst-dust", 100.0, 10_000, 0),
    )
    .unwrap()
    .id;
    conn.execute(
        "UPDATE security_lots SET remaining_quantity=100.000000000001 WHERE buy_transaction_id=?1",
        params![buy_id],
    )
    .unwrap();

    create_transaction_internal(
        &conn,
        make_sell_input("acc-dust", "inst-dust", 100.0, 10_000, 0),
    )
    .unwrap();

    let remaining_after: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining_after, 0.0, "清仓后批次精确归零，不留尘埃残差");
    let active_lots: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE instrument_id='inst-dust' AND remaining_quantity > 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active_lots, 0, "无残余活跃批次");
}

#[test]
fn float_noise_lot_sell_rejected_with_expected_code() {
    let conn = open();
    seed_account(&conn, "acc-msg", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01"],
    );
    seed_instrument(&conn, "inst-msg", "TSLA", "Tesla", "USD", "unknown");
    let buy_id = create_transaction_internal(
        &conn,
        make_buy_input("acc-msg", "inst-msg", 100.0, 10_000, 0),
    )
    .unwrap()
    .id;
    conn.execute(
        "UPDATE security_lots SET remaining_quantity=99.99999999999999 WHERE buy_transaction_id=?1",
        params![buy_id],
    )
    .unwrap();

    let err = create_transaction_internal(
        &conn,
        make_sell_input("acc-msg", "inst-msg", 200.0, 10_000, 0),
    )
    .unwrap_err();
    assert!(
        err.is_code("trade.insufficient-holding"),
        "位噪声批次仍以既定码拒绝，got: {err:?}"
    );
}
