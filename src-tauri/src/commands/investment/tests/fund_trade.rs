//! 场外基金申赎记账测试（命名对齐源码 trade 模块的 fund 语义分支，issue #302 /
//! ADR-0038）：金额权威（确认单整分金额 + 确认份额必填，单价由两者反算）、
//! 每份成本锚定权威金额、卖出 FIFO 匹配与已实现盈亏的舍入不变式（闭合不变式：
//! 全平仓位 Σ已实现盈亏 = Σ卖出金额 − Σ买入金额，精确到分）、编辑全字段替换纠正。

use crate::commands::transactions::{create_transaction_internal, update_transaction_internal};
use crate::models::TransactionInput;
use crate::transaction::amount::TransactionKind;
use rusqlite::{Connection, params};

use super::super::*;
use super::common::*;

/// 插入场外基金标的（type='fund'，市场 unknown——ADR-0038：基金无交易所市场概念）。
fn insert_fund_instrument(conn: &Connection, id: &str, symbol: &str, name: &str) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'fund',?3,'CNY','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, name],
    )
    .unwrap();
}

/// 基金申购输入：确认单整分金额为权威（amount_cents 必填 > 0），单价不提供
/// （由后端反算，wire 上 price_cents = None）。
fn make_fund_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    amount_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Buy,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: None,
        fee_cents: Some(fee_cents),
        idempotency_key: None,
    }
}

/// 基金赎回输入：同申购，金额权威、单价反算。
fn make_fund_sell_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    amount_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Sell,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-20".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: None,
        fee_cents: Some(fee_cents),
        idempotency_key: None,
    }
}

/// 读取一笔交易行金额与买卖明细（单价/手续费）。
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

/// 基金申购：金额权威——行金额恒等于确认单整分金额（不被单价舍入污染），
/// 单价由（金额 − 手续费）÷ 份额反算到万分之一元，每份成本锚定权威金额单次舍入。
/// 数例：申购 987.6543 份、金额 1000.00 元（100_000 分）、手续费 1.50 元（150 分）
/// → 反算净值 = (100_000 − 150) × 100 ÷ 987.6543 = 10_110（1.0110 元，4 位小数）；
/// 每份成本 = 100_000 × 100 ÷ 987.6543 = 10_125（1.0125 元）。
#[test]
fn fund_buy_amount_is_authoritative_and_price_derived() {
    let conn = setup_db();
    insert_account(&conn, "acc-fund", "基金户", "investment", "CNY");
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

/// 每份成本舍入不变式：批次成本重建（数量 × 每份成本）与权威金额的偏差被
/// 单次舍入钉在半价格单位/份以内；全平仓时已实现盈亏按权威金额闭合（不按
/// 重建值漂移）。数例：申购 10_000 份、金额 12345.67 元（1_234_567 分）→ 每份
/// 成本 = round(1_234_567 × 100 ÷ 10_000) = 12_346，重建 1_234_600 分偏差 33 分
///（≤ 10_000 ÷ 2 = 5_000 价格单位）；赎回 10_000 份、金额 13_000.00 元、费 13 元
/// → 已实现盈亏 = 1_300_000 − 1_234_567 = 65_433 分（精确闭合，而非重建值
/// 1_234_600 给出的 65_400）。
#[test]
fn fund_lot_cost_anchors_to_authoritative_amount_and_pnl_closes_exactly() {
    let conn = setup_db();
    insert_account(&conn, "acc-cost", "基金户", "investment", "CNY");
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

/// 基金申购金额必填且为正：金额缺失（0）拒绝，中文报错。
#[test]
fn fund_buy_requires_positive_authoritative_amount() {
    let conn = setup_db();
    insert_account(&conn, "acc-amount", "基金户", "investment", "CNY");
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
    // 不落库残留
    let txns: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(txns, 0, "被拒的申购不应落交易行");
}

/// 基金申购手续费不得超过金额：反算净投入 ≤ 0 无从产生正单价，显式拒绝。
#[test]
fn fund_buy_fee_must_not_exceed_amount() {
    let conn = setup_db();
    insert_account(&conn, "acc-fee", "基金户", "investment", "CNY");
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

/// 基金赎回：金额权威——行金额恒等于确认单整分金额，单价由（金额 + 手续费）
/// ÷ 份额反算；FIFO 匹配与已实现盈亏按权威口径闭合。
/// 数例：买入 500 份金额 500.00 元（每份成本 1.0000 元）；赎回 500 份、金额
/// 520.00 元（52_000 分）、费 0.52 元（52 分）→ 反算净值 = (52_000 + 52) × 100
/// ÷ 500 = 10_410.4 → 10_410（1.0410 元）；盈亏 = 52_000 − 50_000 = 2_000 分。
#[test]
fn fund_sell_amount_is_authoritative_price_derived_and_fifo_pnl_exact() {
    let conn = setup_db();
    insert_account(&conn, "acc-sell", "基金户", "investment", "CNY");
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

/// 闭合不变式（头牌不变式，issue #302）：多批次买入 + 多笔卖出（含一笔跨两批次
/// 的卖出）全平仓后，Σ 已实现盈亏 = Σ 卖出金额 − Σ 买入金额，精确到分——
/// 卖出费在收入与分摊两端同额出现而抵消，买入费摊薄入成本。
///
/// 数例（份额与净值均取万分之一元刻度下不可整除的丑数，暴露一切逐笔舍入）：
/// - 买入①：987.6543 份、金额 100_000 分、费 150 → 每份成本 10_125；
/// - 买入②：500 份、金额 50_000 分、费 0 → 每份成本 10_000；
/// - 卖出①：300 份、金额 31_500 分、费 31 → 净值 (31_500+31)×100÷300 = 10_510.33
///   → 10_510；匹配①仅批次①（未耗尽）：收入 = 金额+费 = 31_531（单匹配兜底）、
///   成本 = round(300×10_125÷100) = 30_375、费 31 → 盈亏 1_125；
/// - 卖出②：1187.6543 份、金额 125_000 分、费 125 → 净值 12_512_500÷1187.6543
///   = 10535.47 → 10_535；匹配②批次①余 687.6543 份（耗尽）：收入
///   round(687.6543×10_535÷100) = 72_444、成本 = 100_000 − 30_375 = 69_625
///   （批次成本按权威金额闭合）、费 floor(125×687.6543÷1187.6543) = 72 → 盈亏 2_747；
///   匹配③批次②500 份（耗尽，兼卖出末匹配）：收入 = 125_125 − 72_444 = 52_681
///   （末匹配吸收收入余数；round 重建为 52_675，差 6 分即本不变式防守的漂移）、
///   成本 50_000、费 53 → 盈亏 2_628；
/// - Σ 盈亏 = 1_125 + 2_747 + 2_628 = 6_500 = (31_500 + 125_000) − (100_000 + 50_000)。
#[test]
fn fund_closed_position_realized_pnl_equals_sell_amounts_minus_buy_amounts() {
    let conn = setup_db();
    insert_account(&conn, "acc-close", "基金户", "investment", "CNY");
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
    // 错开批次 created_at 保证 FIFO 顺序明确（同日亦可，但显式化不依赖落库时序）。
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

/// 基金申赎金额权威与单价权威互斥：wire 误传 price_cents 显式拒绝（不静默吞掉，
/// 与前端装配器「金额与单价不可同时提供」同一契约，issue #302）。
#[test]
fn fund_buy_rejects_price_cents_alongside_authoritative_amount() {
    let conn = setup_db();
    insert_account(&conn, "acc-mutual", "基金户", "investment", "CNY");
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

/// 非基金标的（股票等）保持既有单价权威语义：wire 上的金额字段被忽略，
/// 行金额仍由数量 × 单价 ± 手续费重算（类型分支不误伤既有通道）。
#[test]
fn stock_buy_keeps_price_authoritative_and_ignores_amount_field() {
    let conn = setup_db();
    insert_account(&conn, "acc-stock", "证券户", "investment", "CNY");
    insert_instrument(&conn, "inst-stock", "600519", "贵州茅台", "CNY");

    let mut input = make_buy_input("acc-stock", "inst-stock", 3.0, 12_345, 100);
    input.amount_cents = 999_999; // 非基金：金额字段不具权威语义，应被忽略
    let id = create_transaction_internal(&conn, input).unwrap().id;

    let (amount_cents, _, price_cents, fee_cents) = trade_row(&conn, &id);
    assert_eq!(
        amount_cents, 470,
        "行金额 = 3 × 1.2345 + 1 = 4.70 元（既有口径）"
    );
    assert_eq!(price_cents, 12_345, "单价权威：原样落库");
    assert_eq!(fee_cents, 100);
}

/// 基金申购编辑（全字段替换权威）：改金额/份额/手续费后交易行、明细与批次全部
/// 按新确认单重建，每份成本随新金额重锚。
#[test]
fn fund_buy_edit_replaces_fields_and_reanchors_lot_cost() {
    let conn = setup_db();
    insert_account(&conn, "acc-edit", "基金户", "investment", "CNY");
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

/// 基金赎回编辑（全字段替换权威）：重算匹配与盈亏；编辑后再平仓，闭合不变式
/// 在「编辑过的事实」上依然精确成立。
///
/// 数例：买入 500 份金额 50_000；赎回改报为 400 份、金额 41_000、费 41 → 净值
/// (41_000+41)×100÷400 = 10260.25 → 10_260，盈亏 = 41_041 − 40_000 − 41 = 1_000；
/// 再赎回余 100 份、金额 10_100、费 10 → 收入 10_110、成本 = 50_000 − 40_000
/// = 10_000（批次闭合）、费 10 → 盈亏 100；Σ = 1_100 = 51_100 − 50_000。
#[test]
fn fund_sell_edit_rebuilds_matches_and_closed_invariant_still_holds() {
    let conn = setup_db();
    insert_account(&conn, "acc-sell-edit", "基金户", "investment", "CNY");
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

    // 平掉剩余 100 份：闭合不变式在编辑后的事实上仍精确成立。
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

/// 持仓显形（issue #302 验收 3 的后端腿）：基金持仓进 v_holdings——市值与未实现
/// 盈亏按万分位刻度实时计算，成本锚定权威金额。
#[test]
fn fund_holdings_show_market_value_and_unrealized_pnl() {
    let conn = setup_db();
    insert_account(&conn, "acc-holding", "基金户", "investment", "CNY");
    insert_fund_instrument(&conn, "inst-holding", "000130", "持仓显形基金");

    create_transaction_internal(
        &conn,
        make_fund_buy_input("acc-holding", "inst-holding", 500.0, 50_000, 0),
    )
    .unwrap();

    let now = crate::db::now_iso();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,nav_date,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,10800,'CNY',?3,'2026-01-19',NULL,?4,?5,?6,?7)",
        params![crate::db::new_uuid(), "inst-holding", now, now, now, 1, "test"],
    )
    .unwrap();

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
