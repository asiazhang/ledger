use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{create_transaction_internal, update_transaction_internal};
use rusqlite::{Connection, params};

use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_fund_market_price, seed_instrument};

fn make_fund_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    amount_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        funding: Vec::new(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        funding_account_id: None,
        source_order_no: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: None,
        fee_cents: Some(fee_cents),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

fn make_fund_sell_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    amount_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Sell,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        funding: Vec::new(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        funding_account_id: None,
        source_order_no: None,
        note: None,
        date: "2026-01-20".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: None,
        fee_cents: Some(fee_cents),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

fn trade_row(conn: &Connection, id: &str) -> (i64, f64, i64, i64) {
    conn.query_row(
        "SELECT t.amount_cents, st.quantity, st.price_cents, st.fee_cents \
         FROM transactions t JOIN security_transactions st ON st.transaction_id = t.id \
         WHERE t.id=?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap()
}

#[test]
fn fund_buy_amount_is_authoritative_and_price_derived() {
    let conn = open();
    seed_account(&conn, "acc-fund", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-fund", "000123", "某混合基金");

    let buy_id = create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-fund", "inst-fund", 987.6543, 100_000, 150),
    )
    .unwrap()
    .id;

    let (amount_cents, quantity, price_cents, fee_cents) = trade_row(&conn, &buy_id);
    assert_eq!(
        amount_cents, 100_000,
        "行金额 = 确认单整分金额（权威），不被单价舍入污染（若按价格重算会得 100_002）"
    );
    assert!((quantity - 987.6543).abs() < 1e-9);
    assert_eq!(
        price_cents, 10_110,
        "反算单价 = (100_000 − 150) × 100 ÷ 987.6543 ≈ 10109.81 → 10_110 万分之一元（1.0110 元）"
    );
    assert_eq!(fee_cents, 150);

    let cpu: i64 = conn
        .query_row(
            "SELECT cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        cpu, 10_125,
        "每份成本锚定权威金额单次舍入：100_000 × 100 ÷ 987.6543 ≈ 10125.0002 → 10_125"
    );
}

#[test]
fn fund_lot_cost_anchors_to_authoritative_amount_and_pnl_closes_exactly() {
    let conn = open();
    seed_account(&conn, "acc-cost", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-cost", "000456", "净值保真基金");

    let buy_id = create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-cost", "inst-cost", 10_000.0, 1_234_567, 0),
    )
    .unwrap()
    .id;
    let cpu: i64 = conn
        .query_row(
            "SELECT cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cpu, 12_346);
    let qty = 10_000.0_f64;
    let drift = (qty * cpu as f64 - 1_234_567.0 * 100.0).abs();
    assert!(
        drift <= qty / 2.0,
        "成本重建偏差 {drift} 价格单位应 ≤ 数量/2（单次舍入不变式）"
    );

    let sell_id = create_transaction_internal(
        &conn,
        make_fund_sell_input("acc-cost", "inst-cost", 10_000.0, 1_300_000, 1_300),
    )
    .unwrap()
    .id;
    let (sell_amount, pnl): (i64, i64) = conn
        .query_row(
            "SELECT t.amount_cents, sls.realized_pnl_cents FROM transactions t \
             JOIN security_lot_sales sls ON sls.sell_transaction_id = t.id WHERE t.id=?1",
            params![sell_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(sell_amount, 1_300_000);
    assert_eq!(
        pnl, 65_433,
        "已实现盈亏按权威成本闭合：1_300_000 − 1_234_567 = 65_433（重建值会给 65_400）"
    );
}

#[test]
fn fund_buy_requires_positive_authoritative_amount() {
    let conn = open();
    seed_account(&conn, "acc-amount", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-amount", "000124", "某基金");

    let err = create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-amount", "inst-amount", 100.0, 0, 0),
    )
    .unwrap_err();
    match err {
        AppError::Coded { message, .. } => assert!(
            message.contains("金额"),
            "基金申购缺金额应报中文错误，got: {message}"
        ),
        other => panic!("应返回 Coded（400），got: {other:?}"),
    }
    let txns: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(txns, 0, "被拒的申购不应落交易行");
}

#[test]
fn fund_buy_fee_must_not_exceed_amount() {
    let conn = open();
    seed_account(&conn, "acc-fee", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-fee", "000125", "某基金");

    let err = create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-fee", "inst-fee", 100.0, 1_000, 1_000),
    )
    .unwrap_err();
    match err {
        AppError::Coded { message, .. } => assert!(
            message.contains("手续费"),
            "基金申购手续费 ≥ 金额应报中文错误，got: {message}"
        ),
        other => panic!("应返回 Coded（400），got: {other:?}"),
    }
}

#[test]
fn fund_sell_amount_is_authoritative_price_derived_and_fifo_pnl_exact() {
    let conn = open();
    seed_account(&conn, "acc-sell", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-sell", "000126", "某债券基金");

    let buy_id = create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-sell", "inst-sell", 500.0, 50_000, 0),
    )
    .unwrap()
    .id;
    let sell_id = create_transaction_internal(
        &conn,
        make_fund_sell_input("acc-sell", "inst-sell", 500.0, 52_000, 52),
    )
    .unwrap()
    .id;

    let (amount_cents, _, price_cents, fee_cents) = trade_row(&conn, &sell_id);
    assert_eq!(amount_cents, 52_000, "行金额 = 确认单整分金额（权威）");
    assert_eq!(
        price_cents, 10_410,
        "反算单价 = (52_000 + 52) × 100 ÷ 500 = 10410.4 → 10_410（1.0410 元）"
    );
    assert_eq!(fee_cents, 52);

    let (qty, cost, pnl): (f64, i64, i64) = conn
        .query_row(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents \
             FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!((qty - 500.0).abs() < 1e-9);
    assert_eq!(cost, 10_000, "批次成本快照 = 1.0000 元（万分之一元刻度）");
    assert_eq!(pnl, 2_000, "盈亏 = 卖出金额 − 买入金额 = 52_000 − 50_000");

    let remaining: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!((remaining - 0.0).abs() < 1e-9, "全平仓");
}

#[test]
fn fund_closed_position_realized_pnl_equals_sell_amounts_minus_buy_amounts() {
    let conn = open();
    seed_account(&conn, "acc-close", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-close", "000127", "丑数检验基金");

    let buy1 = create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-close", "inst-close", 987.6543, 100_000, 150),
    )
    .unwrap()
    .id;
    let buy2 = create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-close", "inst-close", 500.0, 50_000, 0),
    )
    .unwrap()
    .id;
    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-10T00:00:00Z' WHERE buy_transaction_id=?1",
        params![buy1],
    )
    .unwrap();
    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-11T00:00:00Z' WHERE buy_transaction_id=?1",
        params![buy2],
    )
    .unwrap();

    let sell1 = create_transaction_internal(
        &conn,
        make_fund_sell_input("acc-close", "inst-close", 300.0, 31_500, 31),
    )
    .unwrap()
    .id;
    let sell2 = create_transaction_internal(
        &conn,
        make_fund_sell_input("acc-close", "inst-close", 1187.6543, 125_000, 125),
    )
    .unwrap()
    .id;

    let pnl1: i64 = conn
        .query_row(
            "SELECT realized_pnl_cents FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pnl1, 1_125, "卖出①单匹配：31_531 − 30_375 − 31");

    let rows: Vec<(f64, i64, i64)> = conn
        .prepare(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents \
             FROM security_lot_sales WHERE sell_transaction_id=?1 ORDER BY created_at ASC",
        )
        .unwrap()
        .query_map(params![sell2], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2, "卖出②跨两批次 FIFO 匹配");
    assert!((rows[0].0 - 687.6543).abs() < 1e-9, "先耗尽批次①");
    assert_eq!(rows[0].2, 2_747, "批次①耗尽匹配：72_444 − 69_625 − 72");
    assert!((rows[1].0 - 500.0).abs() < 1e-9);
    assert_eq!(
        rows[1].2, 2_628,
        "批次②耗尽匹配收入吸收余数：52_681 − 50_000 − 53（round 重建会给 52_675）"
    );

    let total_pnl: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(realized_pnl_cents),0) FROM security_lot_sales",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        total_pnl, 6_500,
        "闭合不变式：Σ 已实现盈亏 = Σ 卖出金额 − Σ 买入金额 = 156_500 − 150_000（精确到分）"
    );
}

#[test]
fn fund_buy_rejects_price_cents_alongside_authoritative_amount() {
    let conn = open();
    seed_account(&conn, "acc-mutual", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-mutual", "000131", "互斥检验基金");

    let mut input = make_fund_buy_input("acc-mutual", "inst-mutual", 100.0, 10_000, 0);
    input.price_cents = Some(1000);
    let err = create_transaction_internal(&conn, input).unwrap_err();
    match err {
        AppError::Coded { message, .. } => assert!(
            message.contains("不可提供单价"),
            "基金申购同供金额与单价应被显式拒绝，got: {message}"
        ),
        other => panic!("应返回 Coded（400），got: {other:?}"),
    }
}

#[test]
fn stock_buy_keeps_price_authoritative_and_ignores_amount_field() {
    let conn = open();
    seed_account(&conn, "acc-stock", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-stock", "600519", "贵州茅台", "CNY", "unknown");

    let mut input = make_buy_input("acc-stock", "inst-stock", 3.0, 12_345, 100);
    input.amount_cents = 999_999;
    let id = create_transaction_internal(&conn, input).unwrap().id;

    let (amount_cents, _, price_cents, fee_cents) = trade_row(&conn, &id);
    assert_eq!(
        amount_cents, 470,
        "行金额 = 3 × 1.2345 + 1 = 4.70 元（既有口径）"
    );
    assert_eq!(price_cents, 12_345, "单价权威：原样落库");
    assert_eq!(fee_cents, 100);
}

#[test]
fn fund_buy_edit_replaces_fields_and_reanchors_lot_cost() {
    let conn = open();
    seed_account(&conn, "acc-edit", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-edit", "000128", "可纠错基金");

    let buy_id = create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-edit", "inst-edit", 500.0, 50_000, 0),
    )
    .unwrap()
    .id;

    let mut edited = make_fund_buy_input("acc-edit", "inst-edit", 600.0, 66_000, 66);
    edited.date = "2026-01-12".into();
    update_transaction_internal(&conn, &buy_id, edited).unwrap();

    let (amount_cents, quantity, price_cents, fee_cents) = trade_row(&conn, &buy_id);
    assert_eq!(amount_cents, 66_000, "编辑后金额权威照抄新确认单");
    assert!((quantity - 600.0).abs() < 1e-9);
    assert_eq!(
        price_cents, 10_989,
        "反算单价 = (66_000 − 66) × 100 ÷ 600 = 10989 → 10_989"
    );
    assert_eq!(fee_cents, 66);

    let (remaining, cpu): (f64, i64) = conn
        .query_row(
            "SELECT remaining_quantity, cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((remaining - 600.0).abs() < 1e-9, "批次按新份额重建");
    assert_eq!(cpu, 11_000, "每份成本随新金额重锚：66_000 × 100 ÷ 600");
}

#[test]
fn fund_sell_edit_rebuilds_matches_and_closed_invariant_still_holds() {
    let conn = open();
    seed_account(&conn, "acc-sell-edit", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-sell-edit", "000129", "纠错赎回基金");

    let _buy_id = create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-sell-edit", "inst-sell-edit", 500.0, 50_000, 0),
    )
    .unwrap()
    .id;
    let sell_id = create_transaction_internal(
        &conn,
        make_fund_sell_input("acc-sell-edit", "inst-sell-edit", 500.0, 52_000, 52),
    )
    .unwrap()
    .id;

    let edited = make_fund_sell_input("acc-sell-edit", "inst-sell-edit", 400.0, 41_000, 41);
    update_transaction_internal(&conn, &sell_id, edited).unwrap();

    let (amount_cents, _, price_cents, _): (i64, f64, i64, i64) = trade_row(&conn, &sell_id);
    assert_eq!(amount_cents, 41_000, "编辑后金额权威照抄新确认单");
    assert_eq!(price_cents, 10_260, "反算净值随编辑重算");
    let (pnl_after_edit, remaining): (i64, f64) = conn
        .query_row(
            "SELECT sls.realized_pnl_cents, l.remaining_quantity FROM security_lot_sales sls \
             JOIN security_lots l ON l.id = sls.lot_id WHERE sls.sell_transaction_id=?1",
            params![sell_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(pnl_after_edit, 1_000, "41_041 − 40_000 − 41");
    assert!((remaining - 100.0).abs() < 1e-9, "持仓随编辑纠正");

    create_transaction_internal(
        &conn,
        make_fund_sell_input("acc-sell-edit", "inst-sell-edit", 100.0, 10_100, 10),
    )
    .unwrap();
    let total_pnl: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(realized_pnl_cents),0) FROM security_lot_sales",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        total_pnl, 1_100,
        "Σ 盈亏 = (41_000 + 10_100) − 50_000 = 1_100（批次成本按权威金额闭合）"
    );
}

#[test]
fn fund_holdings_show_market_value_and_unrealized_pnl() {
    let conn = open();
    seed_account(&conn, "acc-holding", "基金户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-holding", "000130", "持仓显形基金");

    create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-holding", "inst-holding", 500.0, 50_000, 0),
    )
    .unwrap();

    seed_fund_market_price(&conn, "inst-holding", 10_800, "CNY", "2026-01-19", None);

    let (quantity, cost_basis, market_value, unrealized): (f64, i64, i64, i64) = conn
        .query_row(
            "SELECT quantity, cost_basis_cents, market_value_cents, unrealized_pnl_cents \
             FROM v_holdings WHERE instrument_id=?1",
            params!["inst-holding"],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert!((quantity - 500.0).abs() < 1e-9);
    assert_eq!(cost_basis, 50_000, "成本 = 500 × 10_000 ÷ 100 = 50_000 分");
    assert_eq!(
        market_value, 54_000,
        "市值 = 500 × 10_800 ÷ 100 = 54_000 分"
    );
    assert_eq!(unrealized, 4_000, "未实现盈亏 = 54_000 − 50_000");
}
