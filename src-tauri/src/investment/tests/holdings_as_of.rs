use crate::commands::transactions::create_transaction_internal;
use crate::transaction::amount::TransactionKind;
use rusqlite::params;

use super::super::*;
use super::common::*;

// ---------------------------------------------------------------------------
// 时点持仓（AsOfHolding，spec #168 / issue #218）：
// 仅认 buy/sell 流水 · sell 取负 · 按交易日（含当日）前缀求和。
// ---------------------------------------------------------------------------

#[test]
fn holdings_as_of_sums_multiple_buys_and_sells_within_same_week() {
    let conn = setup_db();
    insert_account(&conn, "acc-ao", "证券户", "investment", "CNY");
    insert_instrument(&conn, "inst-ao", "000001", "平安银行", "CNY");
    // 同一周内三笔流水：买 10 → 卖 4 → 买 2（周采样键不影响推算，前缀只认交易日）。
    for (kind, qty, price, date) in [
        (TransactionKind::Buy, 10.0, 1500, "2026-02-04"),
        (TransactionKind::Sell, 4.0, 1600, "2026-02-05"),
        (TransactionKind::Buy, 2.0, 1500, "2026-02-06"),
    ] {
        create_transaction_internal(
            &conn,
            make_trade_input(kind, "acc-ao", "inst-ao", qty, price, date),
        )
        .unwrap();
    }

    // 前缀含当日：02-04 → 10；周末 02-08 → 10−4+2 = 8。
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao"), "2026-02-04").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao"), "2026-02-08").unwrap();
    assert!((qty - 8.0).abs() < 1e-9);

    // as_of 键为交易日（ISO YYYY-MM-DD），格式非法时显式报错（时间键契约显式化）。
    let err = holdings::holdings_as_of(&conn, Some("inst-ao"), "2026/02/08").unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
}

#[test]
fn holdings_as_of_replays_flow_before_query_range_start() {
    // 区间起点前买入（历史时点回放）：前缀求和把查询区间之前的流水累积带入。
    let conn = setup_db();
    insert_account(&conn, "acc-ao2", "证券户", "investment", "CNY");
    insert_instrument(&conn, "inst-ao2", "600519", "贵州茅台", "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ao2",
            "inst-ao2",
            10.0,
            1500,
            "2026-01-05",
        ),
    )
    .unwrap();

    // 查询时点（02-01）晚于买入一个多月：数量仍为 10。
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao2"), "2026-02-01").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    // 早于全部流水的时点：数量 0。
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao2"), "2025-12-31").unwrap();
    assert!((qty - 0.0).abs() < 1e-9);
}

#[test]
fn holdings_as_of_supports_historical_points_after_full_exit() {
    // 已清仓后查历史时点：清仓时点前有持仓、清仓时点后归零。
    let conn = setup_db();
    insert_account(&conn, "acc-ao3", "证券户", "investment", "CNY");
    insert_instrument(&conn, "inst-ao3", "000001", "平安银行", "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ao3",
            "inst-ao3",
            10.0,
            1500,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-ao3",
            "inst-ao3",
            10.0,
            1600,
            "2026-01-20",
        ),
    )
    .unwrap();

    let qty = holdings::holdings_as_of(&conn, Some("inst-ao3"), "2026-01-15").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao3"), "2026-01-25").unwrap();
    assert!((qty - 0.0).abs() < 1e-9);
}

#[test]
fn holdings_as_of_is_currency_agnostic_for_cross_currency_instrument() {
    // 跨币种标的：模块只管数量，币种与折算不进推算。
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "美股户", "investment", "USD");
    insert_instrument(&conn, "inst-usd", "AAPL", "苹果", "USD");
    insert_rate_1_1(&conn, "USD"); // 买卖落库经 Amount 接缝需要当期汇率
    create_transaction_internal(&conn, make_buy_input("acc-usd", "inst-usd", 5.0, 10_000, 0))
        .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-usd", "inst-usd", 2.0, 11_000, 0),
    )
    .unwrap();

    let qty = holdings::holdings_as_of(&conn, Some("inst-usd"), "2026-01-31").unwrap();
    assert!((qty - 3.0).abs() < 1e-9);
}

#[test]
fn holdings_as_of_without_instrument_sums_whole_portfolio() {
    // 全组合形态（instrument_id=None）：所有标的数量之和，与单标的形态同接缝。
    let conn = setup_db();
    insert_account(&conn, "acc-ao4", "证券户", "investment", "CNY");
    insert_instrument(&conn, "inst-a", "000001", "平安银行", "CNY");
    insert_instrument(&conn, "inst-b", "600519", "贵州茅台", "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ao4",
            "inst-a",
            3.0,
            1500,
            "2026-02-04",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ao4",
            "inst-b",
            7.0,
            1500,
            "2026-02-05",
        ),
    )
    .unwrap();

    let qty = holdings::holdings_as_of(&conn, None, "2026-02-06").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    let qty_a = holdings::holdings_as_of(&conn, Some("inst-a"), "2026-02-06").unwrap();
    assert!((qty_a - 3.0).abs() < 1e-9);
}

/// 绑定不变式（spec #168 定案第 6 条 / issue #218）：同一批 buy/sell 流水下，
/// as-of「今天」≡ Holding 数量口径（v_holdings 聚合，即 lots remaining_quantity 之和）。
/// 未来 split 落地改变数量时最先报警的哨兵。
///
/// 本模块口径已排除软删除账户（issue #217 定案，与 v_holdings 一致），不变式对
/// 软删夹具同样成立；含软删账户的组合走势断言（#219 接线后走势经本接缝同口径）
/// 归 issue #247 单测、#248 e2e，夹具此处从简不涉软删。
#[test]
fn holdings_as_of_today_matches_holding_quantity() {
    let conn = setup_db();
    insert_account(&conn, "acc-ao5", "证券户", "investment", "CNY");
    insert_instrument(&conn, "inst-ao5", "000001", "平安银行", "CNY");
    for (kind, qty, price, date) in [
        (TransactionKind::Buy, 10.0, 1500, "2026-01-05"),
        (TransactionKind::Buy, 5.0, 1600, "2026-01-12"),
        (TransactionKind::Sell, 8.0, 1700, "2026-01-18"),
    ] {
        create_transaction_internal(
            &conn,
            make_trade_input(kind, "acc-ao5", "inst-ao5", qty, price, date),
        )
        .unwrap();
    }

    // as-of「今天」（取晚于全部流水的交易日）= v_holdings 数量 = 10+5−8 = 7。
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao5"), "2026-06-01").unwrap();
    let holding_qty: Option<f64> = conn
        .query_row(
            "SELECT SUM(quantity) FROM v_holdings WHERE instrument_id=?1",
            params!["inst-ao5"],
            |r| r.get(0),
        )
        .unwrap();
    assert!((qty - 7.0).abs() < 1e-9, "as-of = {qty}");
    assert!(
        holding_qty.is_some_and(|q| (q - qty).abs() < 1e-9),
        "Holding = {holding_qty:?}"
    );

    // 清仓后：v_holdings 无行，as-of「今天」归零——两侧仍然一致。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-ao5",
            "inst-ao5",
            7.0,
            1700,
            "2026-01-20",
        ),
    )
    .unwrap();
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao5"), "2026-06-01").unwrap();
    let holding_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id=?1",
            params!["inst-ao5"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(holding_rows, 0);
    assert!((qty - 0.0).abs() < 1e-9, "清仓后 as-of = {qty}");
}
