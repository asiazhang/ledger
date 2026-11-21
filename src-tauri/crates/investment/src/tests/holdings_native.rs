use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{
    open, seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_instrument,
    seed_market_price,
};

#[test]
fn holdings_native_columns_identity_for_base_currency_account() {
    let conn = open();
    seed_account(&conn, "acc-cny", "境内账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-cny", "510300", "沪深300ETF", "CNY", "sh");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cny",
            "inst-cny",
            10.0,
            1_000_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    seed_market_price(&conn, "inst-cny", 1_200_000, "CNY");

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].market_value_cents, Some(120_000));
    assert_eq!(holdings[0].native_market_value_cents, Some(120_000));
    assert_eq!(holdings[0].unrealized_pnl_cents, Some(20_000));
    assert_eq!(holdings[0].native_unrealized_pnl_cents, Some(20_000));
}

#[test]
fn holdings_native_columns_convert_at_current_rate() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2026-01-10"]);
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_instrument(&conn, "inst-usd", "AAPL", "Apple", "USD", "nasdaq");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-usd", "inst-usd", 2.0, 500_000, 0),
    )
    .unwrap();
    seed_market_price(&conn, "inst-usd", 600_000, "USD");

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].market_value_cents, Some(12_000));
    assert_eq!(holdings[0].native_market_value_cents, Some(84_000));
    assert_eq!(holdings[0].unrealized_pnl_cents, Some(2_000));
    assert_eq!(holdings[0].native_unrealized_pnl_cents, Some(14_000));
}

#[test]
fn holdings_native_columns_missing_rate_soft_null() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2026-01-10"]);
    seed_instrument(&conn, "inst-usd", "AAPL", "Apple", "USD", "nasdaq");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-usd", "inst-usd", 2.0, 500_000, 0),
    )
    .unwrap();
    seed_market_price(&conn, "inst-usd", 600_000, "USD");

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].market_value_cents, Some(12_000));
    assert_eq!(holdings[0].unrealized_pnl_cents, Some(2_000));
    assert_eq!(holdings[0].native_market_value_cents, None);
    assert_eq!(holdings[0].native_unrealized_pnl_cents, None);
}

#[test]
fn holdings_native_columns_missing_price_stay_null() {
    let conn = open();
    seed_account(&conn, "acc-cny", "境内账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-cny", "510300", "沪深300ETF", "CNY", "sh");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cny",
            "inst-cny",
            10.0,
            1_000_000,
            "2026-01-10",
        ),
    )
    .unwrap();

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].market_value_cents, None);
    assert_eq!(holdings[0].native_market_value_cents, None);
    assert_eq!(holdings[0].unrealized_pnl_cents, None);
    assert_eq!(holdings[0].native_unrealized_pnl_cents, None);
}

#[test]
fn cumulative_pnl_native_total_converts_legs_at_current_rate() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_instrument(&conn, "inst-usd", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-usd", "inst-usd", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-usd", "inst-usd", 5.0, 1_200_000, 200),
    )
    .unwrap();
    seed_market_price(&conn, "inst-usd", 1_100_000, "USD");
    seed_account(&conn, "acc-cny", "A 股账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-cny", "510300", "沪深300ETF", "CNY", "sh");
    create_transaction_internal(
        &conn,
        make_dividend_input("acc-cny", "inst-cny", 2_000, "CNY"),
    )
    .unwrap();

    let total = query_cumulative_pnl_native_total(&conn).unwrap();
    assert_eq!(total.native_currency, "CNY");
    assert_eq!(total.total_cents, 14_800 * 7 + 2_000);
}

#[test]
fn cumulative_pnl_native_total_includes_hidden_accounts() {
    let conn = open();
    seed_account(&conn, "acc-h", "隐藏投资账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-h", "HID", "隐藏标的", "CNY", "sh");
    create_transaction_internal(&conn, make_dividend_input("acc-h", "inst-h", 4_000, "CNY"))
        .unwrap();
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-h'", [])
        .unwrap();

    let total = query_cumulative_pnl_native_total(&conn).unwrap();
    assert_eq!(total.total_cents, 4_000, "隐藏账户的分红腿照常计入");
}

#[test]
fn cumulative_pnl_native_total_missing_rate_raises_coded_error() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2026-02-10"]);
    seed_instrument(&conn, "inst-usd", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        make_dividend_input("acc-usd", "inst-usd", 1_000, "USD"),
    )
    .unwrap();

    let err = query_cumulative_pnl_native_total(&conn).unwrap_err();
    assert!(
        err.is_code("fx.rate-missing"),
        "缺汇率应报 fx.rate-missing，实际 {err:?}"
    );
}

#[test]
fn cumulative_pnl_native_total_skips_unpriced_holding() {
    let conn = open();
    seed_account(&conn, "acc-cny", "境内账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-cny", "510300", "沪深300ETF", "CNY", "sh");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cny",
            "inst-cny",
            10.0,
            1_000_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input("acc-cny", "inst-cny", 2_500, "CNY"),
    )
    .unwrap();

    let total = query_cumulative_pnl_native_total(&conn).unwrap();
    assert_eq!(total.total_cents, 2_500);
}
