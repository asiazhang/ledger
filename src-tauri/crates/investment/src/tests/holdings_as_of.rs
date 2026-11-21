use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;
use rusqlite::{Connection, params};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_fx_history_weeks, seed_instrument};

#[test]
fn holdings_as_of_sums_multiple_buys_and_sells_within_same_week() {
    let conn = open();
    seed_account(&conn, "acc-ao", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ao", "000001", "平安银行", "CNY", "unknown");
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

    let qty = holdings::holdings_as_of(&conn, Some("inst-ao"), "2026-02-04").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao"), "2026-02-08").unwrap();
    assert!((qty - 8.0).abs() < 1e-9);

    let err = holdings::holdings_as_of(&conn, Some("inst-ao"), "2026/02/08").unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
}

#[test]
fn holdings_as_of_replays_flow_before_query_range_start() {
    let conn = open();
    seed_account(&conn, "acc-ao2", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ao2", "600519", "贵州茅台", "CNY", "unknown");
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

    let qty = holdings::holdings_as_of(&conn, Some("inst-ao2"), "2026-02-01").unwrap();
    assert!((qty - 10.0).abs() < 1e-9);
    let qty = holdings::holdings_as_of(&conn, Some("inst-ao2"), "2025-12-31").unwrap();
    assert!((qty - 0.0).abs() < 1e-9);
}

#[test]
fn holdings_as_of_supports_historical_points_after_full_exit() {
    let conn = open();
    seed_account(&conn, "acc-ao3", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ao3", "000001", "平安银行", "CNY", "unknown");
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
    let conn = open();
    seed_account(&conn, "acc-usd", "美股户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-usd", "AAPL", "苹果", "USD", "unknown");
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
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
    let conn = open();
    seed_account(&conn, "acc-ao4", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-a", "000001", "平安银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-b", "600519", "贵州茅台", "CNY", "unknown");
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

#[test]
fn first_position_date_takes_earliest_leg_date() {
    let conn = open();
    seed_account(&conn, "acc-fp", "证券户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-fp",
        "000025",
        "大摩双利增强债券C",
        "CNY",
        "unknown",
    );
    for (kind, qty, price, date) in [
        (TransactionKind::Buy, 10.0, 1500, "2020-06-10"),
        (TransactionKind::Buy, 5.0, 1600, "2021-03-04"),
        (TransactionKind::Sell, 3.0, 1700, "2022-08-18"),
    ] {
        create_transaction_internal(
            &conn,
            make_trade_input(kind, "acc-fp", "inst-fp", qty, price, date),
        )
        .unwrap();
    }

    let first = holdings::first_position_date(&conn, "inst-fp").unwrap();
    assert_eq!(first.as_deref(), Some("2020-06-10"), "首笔腿 = 最早交易日");
}

#[test]
fn first_position_date_counts_convert_legs_and_split() {
    let conn = open();
    seed_account(&conn, "acc-fp2", "证券户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-fp-out",
        "000024.OF",
        "转出基金",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-fp-in",
        "000025.OF",
        "转入基金",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-fp2",
            "inst-fp-out",
            10.0,
            1500,
            "2025-12-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_convert_input(
            "acc-fp2",
            "inst-fp-out",
            "inst-fp-in",
            10.0,
            12.0,
            15_000,
            15_000,
            0,
        ),
    )
    .unwrap();

    let out = holdings::first_position_date(&conn, "inst-fp-out").unwrap();
    let inp = holdings::first_position_date(&conn, "inst-fp-in").unwrap();
    assert_eq!(out.as_deref(), Some("2025-12-01"), "转出标的 = 前置买入日");
    assert_eq!(
        inp.as_deref(),
        Some("2026-02-01"),
        "转入标的 = 转换日（转入腿命中）"
    );
}

#[test]
fn first_position_date_excludes_dividend_and_soft_deleted_rows() {
    let conn = open();
    seed_account(&conn, "acc-fp3", "证券户", "investment", "CNY", 0);
    seed_account(&conn, "acc-fp3-gone", "待删户", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-fp-div",
        "000026",
        "仅分红标的",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-fp-gone",
        "000027",
        "软删户标的",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-fp-del",
        "000028",
        "软删流水标的",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-fp3", "inst-fp-div", 1_000, "CNY", "2026-01-05"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-fp3-gone",
            "inst-fp-gone",
            10.0,
            1500,
            "2026-01-06",
        ),
    )
    .unwrap();
    let mut deleted_buy = make_trade_input(
        TransactionKind::Buy,
        "acc-fp3",
        "inst-fp-del",
        2.0,
        1500,
        "2019-01-07",
    );
    deleted_buy.note = Some("fp-to-delete".into());
    create_transaction_internal(&conn, deleted_buy).unwrap();
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE note='fp-to-delete'",
        [],
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-fp3",
            "inst-fp-del",
            8.0,
            1500,
            "2026-01-08",
        ),
    )
    .unwrap();

    assert_eq!(
        holdings::first_position_date(&conn, "inst-fp-div").unwrap(),
        None,
        "仅分红（零份额变动）标的无首笔持仓腿"
    );
    conn.execute(
        "UPDATE accounts SET is_deleted=1 WHERE id='acc-fp3-gone'",
        [],
    )
    .unwrap();
    assert_eq!(
        holdings::first_position_date(&conn, "inst-fp-gone").unwrap(),
        None,
        "软删账户的买入不进判"
    );
    assert_eq!(
        holdings::first_position_date(&conn, "inst-fp-del")
            .unwrap()
            .as_deref(),
        Some("2026-01-08"),
        "软删流水不进判，取在场最早腿"
    );
}

#[test]
fn first_position_date_none_for_instrument_without_legs() {
    let conn = open();
    seed_instrument(
        &conn,
        "inst-fp-none",
        "000029",
        "无流水标的",
        "CNY",
        "unknown",
    );
    assert_eq!(
        holdings::first_position_date(&conn, "inst-fp-none").unwrap(),
        None,
        "无持仓流水 → None（覆盖目标维持近两年）"
    );
}

#[test]
fn holdings_as_of_today_matches_holding_quantity() {
    let conn = open();
    seed_account(&conn, "acc-ao5", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ao5", "000001", "平安银行", "CNY", "unknown");
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

#[test]
fn holdings_legs_stream_prefix_sums_match_as_of_on_mixed_fixture() {
    let conn = open();
    seed_account(&conn, "acc-lg", "证券户", "investment", "CNY", 0);
    seed_account(&conn, "acc-lg-gone", "待删户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-lg-a", "000001", "平安银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-lg-b", "600036", "招商银行", "CNY", "unknown");
    seed_instrument(
        &conn,
        "inst-lg-out",
        "000001.OF",
        "转出基金",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-lg-in",
        "000002.OF",
        "转入基金",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-lg-none",
        "000008",
        "无腿标的",
        "CNY",
        "unknown",
    );

    for (kind, qty, price, date) in [
        (TransactionKind::Buy, 10.0, 1500, "2026-02-04"),
        (TransactionKind::Sell, 4.0, 1600, "2026-02-05"),
        (TransactionKind::Buy, 2.0, 1500, "2026-02-06"),
    ] {
        create_transaction_internal(
            &conn,
            make_trade_input(kind, "acc-lg", "inst-lg-a", qty, price, date),
        )
        .unwrap();
    }
    let mut deleted_buy = make_trade_input(
        TransactionKind::Buy,
        "acc-lg",
        "inst-lg-a",
        2.0,
        1500,
        "2026-02-07",
    );
    deleted_buy.note = Some("legs-to-delete".into());
    create_transaction_internal(&conn, deleted_buy).unwrap();
    let mut split = make_split_input("acc-lg", "inst-lg-a", 3.0);
    split.date = "2026-02-08".into();
    create_transaction_internal(&conn, split).unwrap();

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-lg-gone",
            "inst-lg-b",
            5.0,
            1500,
            "2026-02-04",
        ),
    )
    .unwrap();

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-lg",
            "inst-lg-out",
            6.0,
            1500,
            "2026-02-03",
        ),
    )
    .unwrap();

    let mut convert = make_convert_input(
        "acc-lg",
        "inst-lg-out",
        "inst-lg-in",
        6.0,
        12.0,
        10_000,
        10_000,
        0,
    );
    convert.date = "2026-02-10".into();
    create_transaction_internal(&conn, convert).unwrap();

    let check_pairing = |conn: &Connection, label: &str| {
        let stream = holdings::holdings_legs_by_instrument(conn).unwrap();
        assert!(
            !stream.contains_key("inst-lg-none"),
            "无腿标的不进腿流（{label}）"
        );
        for (instrument_id, legs) in &stream {
            let dates: Vec<&str> = legs.iter().map(|(d, _)| d.as_str()).collect();
            let mut sorted = dates.clone();
            sorted.sort_unstable();
            assert_eq!(
                dates, sorted,
                "{instrument_id} 腿流应按交易日升序（{label}）"
            );
            let mut running = 0.0f64;
            for (date, qty) in legs {
                running += qty;
                let as_of = holdings::holdings_as_of(conn, Some(instrument_id), date).unwrap();
                assert!(
                    (running - as_of).abs() < 1e-9,
                    "{label}：{instrument_id} @{date} 腿流前缀和 {running} ≠ as-of {as_of}"
                );
            }
            let first = holdings::first_position_date(conn, instrument_id).unwrap();
            assert_eq!(
                first.as_deref(),
                Some(legs[0].0.as_str()),
                "{label}：{instrument_id} 首笔腿 ≠ 腿流首行"
            );
        }
        assert!(
            !holdings::first_position_date(conn, "inst-lg-none")
                .unwrap()
                .is_some(),
            "无腿标的无首笔持仓腿"
        );
        stream
    };

    let stream = check_pairing(&conn, "基线");
    assert_eq!(
        stream["inst-lg-a"].len(),
        5,
        "买 10 + 卖 −4 + 买 2 + 待删买 2 + split Δ3"
    );
    assert_eq!(stream["inst-lg-b"].len(), 1);
    assert_eq!(stream["inst-lg-out"].len(), 2, "前置买入 +6 与转出腿 −6");
    assert_eq!(stream["inst-lg-in"].len(), 1, "转入腿 +12");
    assert_eq!(
        stream["inst-lg-in"][0].1, 12.0,
        "convert 转入腿取 to_quantity"
    );

    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE note='legs-to-delete'",
        [],
    )
    .unwrap();
    let stream = check_pairing(&conn, "软删一笔买入后");
    assert_eq!(stream["inst-lg-a"].len(), 4, "软删买入的腿不再投影");

    conn.execute(
        "UPDATE accounts SET is_deleted=1 WHERE id='acc-lg-gone'",
        [],
    )
    .unwrap();
    let stream = check_pairing(&conn, "软删账户后");
    assert!(!stream.contains_key("inst-lg-b"), "软删账户的腿流整体消失");
    let qty = holdings::holdings_as_of(&conn, Some("inst-lg-b"), "2026-02-06").unwrap();
    assert!((qty - 0.0).abs() < 1e-9, "as-of 同步排除软删账户");
}
