use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{
    create_transaction_internal, delete_transaction_internal, update_transaction_internal,
};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{
    open, seed_account, seed_fx_history_weeks, seed_instrument, seed_market_price,
};

fn cumulative_for(groups: &[CurrencyCumulativePnl], currency: &str) -> i64 {
    groups
        .iter()
        .find(|g| g.currency_code == currency)
        .map(|g| g.cumulative_pnl_cents)
        .unwrap_or(0)
}

fn legs_of(conn: &rusqlite::Connection) -> (i64, i64) {
    let unrealized: i64 = list_holdings(conn)
        .unwrap()
        .iter()
        .filter_map(|h| h.unrealized_pnl_cents)
        .sum();
    let realized: i64 = query_realized_pnl_summary(
        conn,
        &PnlFilter {
            account_id: None,
            instrument_id: None,
        },
    )
    .unwrap()
    .by_year
    .iter()
    .map(|r| r.realized_pnl_cents.unwrap_or(0))
    .sum();
    (unrealized, realized)
}

#[test]
fn cumulative_pnl_equals_unrealized_plus_realized_after_partial_sell() {
    let conn = open();
    seed_account(&conn, "acc-cp", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-cp", "AAPL", "Apple", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-cp", "inst-cp", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-cp", "inst-cp", 5.0, 1_200_000, 200),
    )
    .unwrap();
    seed_market_price(&conn, "inst-cp", 1_100_000, "USD");

    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(realized, 9800);
    assert_eq!(unrealized, 5000);

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(cumulative_for(&groups, "USD"), unrealized + realized);
    assert_eq!(cumulative_for(&groups, "USD"), 14800);
}

#[test]
fn cumulative_pnl_after_full_liquidation_equals_realized() {
    let conn = open();
    seed_account(&conn, "acc-cp", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-cp", "AAPL", "Apple", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-cp", "inst-cp", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-cp", "inst-cp", 10.0, 1_200_000, 0),
    )
    .unwrap();

    assert!(list_holdings(&conn).unwrap().is_empty());
    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(unrealized, 0);
    assert_eq!(realized, 20000);

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "USD"), unrealized + realized);
    assert_eq!(cumulative_for(&groups, "USD"), 20000);
}

#[test]
fn cumulative_pnl_groups_by_currency_without_mixing() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_account(&conn, "acc-cny", "A 股账户", "investment", "CNY", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-usd", "USDX", "USDX Corp", "USD", "unknown");
    seed_instrument(&conn, "inst-cny", "CNYX", "CNYX Corp", "CNY", "unknown");

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

    create_transaction_internal(
        &conn,
        make_buy_input("acc-cny", "inst-cny", 5.0, 100_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-cny", "inst-cny", 2.0, 60_000, 0),
    )
    .unwrap();
    seed_market_price(&conn, "inst-cny", 100_000, "CNY");

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(cumulative_for(&groups, "USD"), 14800);
    assert_eq!(cumulative_for(&groups, "CNY"), -800);
    assert!(groups.iter().all(|g| g.cumulative_pnl_cents != 14000));
}

#[test]
fn cumulative_pnl_skips_holding_without_price() {
    let conn = open();
    seed_account(&conn, "acc-nop", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-nop", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-nop", "inst-nop", 10.0, 1_000_000, 0),
    )
    .unwrap();

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert!(holdings[0].unrealized_pnl_cents.is_none());
    assert!(query_cumulative_pnl_summary(&conn).unwrap().is_empty());
}

#[test]
fn cumulative_pnl_skips_holding_without_fx_rate() {
    let conn = open();
    seed_account(&conn, "acc-fx", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-fx", "7203", "Toyota", "JPY", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-fx", "inst-fx", 10.0, 1_000_000, 0),
    )
    .unwrap();
    seed_market_price(&conn, "inst-fx", 1_100_000, "JPY");

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert!(holdings[0].market_value_cents.is_none());
    assert!(holdings[0].unrealized_pnl_cents.is_none());
    assert!(query_cumulative_pnl_summary(&conn).unwrap().is_empty());
}

#[test]
fn cumulative_pnl_identity_holds_after_convert() {
    let conn = open();
    seed_account(&conn, "acc-cv", "基金户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-out", "006793", "转出基金", "CNY", "unknown");
    seed_instrument(&conn, "inst-in", "519700", "转入基金", "CNY", "unknown");

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
        make_convert_input("acc-cv", "inst-out", "inst-in", 4.0, 4.0, 400, 400, 0),
    )
    .unwrap();
    seed_market_price(&conn, "inst-out", 10_000, "CNY");
    seed_market_price(&conn, "inst-in", 12_000, "CNY");

    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(realized, 0);
    assert_eq!(unrealized, 80);

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "CNY"), unrealized + realized);
    assert_eq!(cumulative_for(&groups, "CNY"), 80);
}

#[test]
fn cumulative_pnl_identity_holds_after_split() {
    let conn = open();
    seed_account(&conn, "acc-sp", "股票户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-sp", "502010", "证券基金", "CNY", "unknown");

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
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0)).unwrap();
    seed_market_price(&conn, "inst-sp", 12_000, "CNY");

    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(realized, 0);
    assert_eq!(unrealized, 14000);

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "CNY"), unrealized + realized);
    assert_eq!(cumulative_for(&groups, "CNY"), 14000);
}

#[test]
fn cumulative_pnl_includes_dividend_as_third_leg() {
    let conn = open();
    seed_account(&conn, "acc-dv", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-dv", "AAPL", "Apple", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-dv", "inst-dv", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-dv", "inst-dv", 5.0, 1_200_000, 200),
    )
    .unwrap();
    seed_market_price(&conn, "inst-dv", 1_100_000, "USD");

    let (unrealized, realized) = legs_of(&conn);
    assert_eq!(unrealized, 5000);
    assert_eq!(realized, 9800);
    assert_eq!(
        cumulative_for(&query_cumulative_pnl_summary(&conn).unwrap(), "USD"),
        unrealized + realized
    );

    for _ in 0..3 {
        create_transaction_internal(&conn, make_dividend_input("acc-dv", "inst-dv", 1000, "USD"))
            .unwrap();
    }

    let (unrealized_after, realized_after) = legs_of(&conn);
    assert_eq!((unrealized_after, realized_after), (unrealized, realized));
    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "USD"), unrealized + realized + 3000);
    assert_eq!(cumulative_for(&groups, "USD"), 17800);
}

#[test]
fn cumulative_pnl_dividend_leg_survives_missing_price() {
    let conn = open();
    seed_account(&conn, "acc-dvp", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-dvp", "AAPL", "Apple", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-dvp", "inst-dvp", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dvp", "inst-dvp", 2500, "USD"),
    )
    .unwrap();

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(cumulative_for(&groups, "USD"), 2500);
}

#[test]
fn cumulative_pnl_dividend_without_holding_is_allowed() {
    let conn = open();
    seed_account(&conn, "acc-dvc", "股票户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dvc", "502010", "证券基金", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-dvc",
            "inst-dvc",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-dvc",
            "inst-dvc",
            100.0,
            12_000,
            "2026-01-20",
        ),
    )
    .unwrap();
    assert!(list_holdings(&conn).unwrap().is_empty());

    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dvc", "inst-dvc", 4000, "CNY"),
    )
    .unwrap();

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(cumulative_for(&groups, "CNY"), 6000);
}

#[test]
fn cumulative_pnl_dividend_update_and_delete_roll_back_precisely() {
    let conn = open();
    seed_account(&conn, "acc-dvr", "股票户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dvr", "502010", "证券基金", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-dvr",
            "inst-dvr",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    seed_market_price(&conn, "inst-dvr", 10_000, "CNY");

    let dividend_id = create_transaction_internal(
        &conn,
        make_dividend_input("acc-dvr", "inst-dvr", 3000, "CNY"),
    )
    .unwrap()
    .id;
    assert_eq!(
        cumulative_for(&query_cumulative_pnl_summary(&conn).unwrap(), "CNY"),
        3000
    );

    update_transaction_internal(
        &conn,
        &dividend_id,
        make_dividend_input("acc-dvr", "inst-dvr", 5000, "CNY"),
    )
    .unwrap();
    assert_eq!(
        cumulative_for(&query_cumulative_pnl_summary(&conn).unwrap(), "CNY"),
        5000
    );

    delete_transaction_internal(&conn, &dividend_id).unwrap();
    assert_eq!(
        cumulative_for(&query_cumulative_pnl_summary(&conn).unwrap(), "CNY"),
        0
    );
}

#[test]
fn cumulative_pnl_dividend_groups_by_currency_without_mixing() {
    let conn = open();
    seed_account(&conn, "acc-dv-usd", "美股账户", "investment", "USD", 0);
    seed_account(&conn, "acc-dv-cny", "A 股账户", "investment", "CNY", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-dv-usd", "USDX", "USDX Corp", "USD", "unknown");
    seed_instrument(&conn, "inst-dv-cny", "CNYX", "CNYX Corp", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv-usd", "inst-dv-usd", 1000, "USD"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv-cny", "inst-dv-cny", 2000, "CNY"),
    )
    .unwrap();

    let groups = query_cumulative_pnl_summary(&conn).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(cumulative_for(&groups, "USD"), 1000);
    assert_eq!(cumulative_for(&groups, "CNY"), 2000);
    assert!(groups.iter().all(|g| g.cumulative_pnl_cents != 3000));
}
