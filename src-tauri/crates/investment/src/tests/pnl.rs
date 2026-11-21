use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_fx_history_weeks, seed_instrument};

fn empty_filter() -> PnlFilter {
    PnlFilter {
        account_id: None,
        instrument_id: None,
    }
}

fn assert_row(row: &YearPnl, year: &str, realized: i64, dividend: i64, gain: i64, native: &str) {
    assert_eq!(row.year, year);
    assert_eq!(row.native_currency, native);
    assert_eq!(row.realized_pnl_cents, Some(realized), "已实现盈亏腿");
    assert_eq!(row.dividend_cents, Some(dividend), "现金分红腿");
    assert_eq!(row.realized_gain_cents, Some(gain), "主值 = 两腿之和");
}

#[test]
fn realized_pnl_summary_empty_when_no_sales() {
    let conn = open();
    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert!(result.by_year.is_empty());
    assert!(result.by_account.is_empty());
}

#[test]
fn realized_pnl_summary_aggregates_single_sale() {
    let conn = open();
    seed_account(&conn, "acc-pnl", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-pnl", "AAPL", "Apple", "USD", "unknown");

    let _buy = create_transaction_internal(
        &conn,
        make_buy_input("acc-pnl", "inst-pnl", 10.0, 1_000_000, 0),
    )
    .unwrap()
    .id;
    let _sell = create_transaction_internal(
        &conn,
        make_sell_input("acc-pnl", "inst-pnl", 5.0, 1_200_000, 200),
    )
    .unwrap()
    .id;

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.by_year.len(), 1);
    assert_row(&result.by_year[0], "2026", 9800, 0, 9800, "CNY");
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-pnl");
    assert_eq!(result.by_account[0].native_currency, "CNY");
    assert_eq!(result.by_account[0].realized_pnl_cents, Some(9800));
    assert_eq!(result.by_account[0].dividend_cents, Some(0));
    assert_eq!(result.by_account[0].realized_gain_cents, Some(9800));
}

#[test]
fn realized_pnl_summary_converts_each_leg_at_its_event_week() {
    let conn = open();
    seed_account(&conn, "acc-week", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-week", "AAPL", "Apple", "USD", "unknown");
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2026-01-10", "2026-01-20"]);
    seed_fx_history_weeks(&conn, "USD", "CNY", 6.0, &["2026-02-01"]);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.5, &["2026-02-10"]);

    create_transaction_internal(
        &conn,
        make_buy_input("acc-week", "inst-week", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-week", "inst-week", 5.0, 1_200_000, 200),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-week", "inst-week", 100_000, "USD", "2026-02-01"),
    )
    .unwrap();
    create_transaction_internal(&conn, {
        let mut input = make_sell_input("acc-week", "inst-week", 5.0, 1_300_000, 0);
        input.date = "2026-02-10".into();
        input
    })
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.by_year.len(), 1);
    assert_row(&result.by_year[0], "2026", 181_100, 600_000, 781_100, "CNY");
}

#[test]
fn realized_pnl_summary_nearest_week_fallback_wired_into_projection() {
    let conn = open();
    seed_account(&conn, "acc-near", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-near", "AAPL", "Apple", "USD", "unknown");
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
        &["2026-01-10", "2026-01-13", "2026-01-20"],
    );

    create_transaction_internal(
        &conn,
        make_buy_input("acc-near", "inst-near", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-near", "inst-near", 5.0, 1_200_000, 200),
    )
    .unwrap();

    conn.execute(
        "DELETE FROM fx_rate_history WHERE trade_date='2026-01-19'",
        [],
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.by_year.len(), 1);
    assert_row(&result.by_year[0], "2026", 68_600, 0, 68_600, "CNY");
}

#[test]
fn realized_pnl_summary_missing_leg_beyond_window_marks_row_not_computable() {
    let conn = open();
    seed_account(&conn, "acc-gap", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-gap", "AAPL", "Apple", "USD", "unknown");
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
        &["2025-06-10", "2026-01-10", "2026-01-20"],
    );

    create_transaction_internal(
        &conn,
        make_buy_input("acc-gap", "inst-gap", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-gap", "inst-gap", 5.0, 1_200_000, 200),
    )
    .unwrap();

    conn.execute(
        "DELETE FROM fx_rate_history WHERE trade_date IN ('2026-01-05','2026-01-19')",
        [],
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(
        result.by_year.len(),
        1,
        "缺料行仍在行集内，显式标注而非消失"
    );
    let row = &result.by_year[0];
    assert_eq!(row.year, "2026");
    assert_eq!(
        row.realized_gain_cents, None,
        "主值必须为 None（无法计算），绝不以零计入"
    );
    assert_eq!(row.realized_pnl_cents, None, "缺料腿为 None，不给半截数字");
    assert_eq!(row.dividend_cents, None, "任一腿缺料即整行无法计算");
    assert_eq!(result.by_account[0].realized_gain_cents, None);
}

#[test]
fn realized_pnl_summary_pair_without_any_history_raises_rate_missing() {
    let conn = open();
    seed_account(&conn, "acc-zero", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-zero", "AAPL", "Apple", "USD", "unknown");
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2026-01-10", "2026-01-20"]);

    create_transaction_internal(
        &conn,
        make_buy_input("acc-zero", "inst-zero", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-zero", "inst-zero", 5.0, 1_200_000, 200),
    )
    .unwrap();

    conn.execute("DELETE FROM fx_rate_history", []).unwrap();

    let err = query_realized_pnl_summary(&conn, &empty_filter()).unwrap_err();
    assert!(
        err.is_code("fx.rate-missing"),
        "币对零历史应报 fx.rate-missing，实际 {err:?}"
    );
}

#[test]
fn realized_pnl_summary_same_currency_identity_without_any_fx() {
    let conn = open();
    seed_account(&conn, "acc-idn", "A 股账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-idn", "600000", "浦发银行", "CNY", "sh");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-idn", "inst-idn", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-idn", "inst-idn", 5.0, 1_200_000, 200),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.by_year.len(), 1);
    assert_row(&result.by_year[0], "2026", 9800, 0, 9800, "CNY");
}

#[test]
fn realized_pnl_summary_row_per_year_not_per_currency() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_account(&conn, "acc-cny", "A 股账户", "investment", "CNY", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
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

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.by_year.len(), 1);
    assert_row(&result.by_year[0], "2026", 67_800, 0, 67_800, "CNY");
    assert_eq!(result.by_account.len(), 2);
    assert_eq!(result.by_account[0].account_id, "acc-cny");
    assert_eq!(result.by_account[0].realized_gain_cents, Some(-800));
    assert_eq!(result.by_account[1].account_id, "acc-usd");
    assert_eq!(result.by_account[1].realized_gain_cents, Some(68_600));
}

#[test]
fn realized_pnl_summary_dividend_only_year_appears() {
    let conn = open();
    seed_account(&conn, "acc-dv", "且慢", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dv", "502010", "证券基金", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-dv", "inst-dv", 836_536, "CNY", "2019-12-31"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.by_year.len(), 1);
    assert_row(&result.by_year[0], "2019", 0, 836_536, 836_536, "CNY");
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-dv");
    assert_eq!(result.by_account[0].realized_gain_cents, Some(836_536));
}

#[test]
fn realized_pnl_summary_merges_both_legs_in_same_year() {
    let conn = open();
    seed_account(&conn, "acc-both", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-both", "501000", "基金A", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-both",
            "inst-both",
            10.0,
            1_000_000,
            "2021-05-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-both",
            "inst-both",
            5.0,
            1_200_000,
            "2021-06-20",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-both", "inst-both", 126_318, "CNY", "2021-12-31"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.by_year.len(), 1);
    assert_row(&result.by_year[0], "2021", 10_000, 126_318, 136_318, "CNY");
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].realized_gain_cents, Some(136_318));
}

#[test]
fn realized_pnl_summary_orders_years_ascending_and_accounts_by_name() {
    let conn = open();
    seed_account(&conn, "acc-b", "账户B", "investment", "CNY", 0);
    seed_account(&conn, "acc-a", "账户A", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-o", "600010", "基金O", "CNY", "sh");

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-b",
            "inst-o",
            10.0,
            1_000_000,
            "2020-05-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-b",
            "inst-o",
            5.0,
            1_200_000,
            "2021-06-20",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-a", "inst-o", 50_000, "CNY", "2020-08-01"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    let years: Vec<&str> = result.by_year.iter().map(|r| r.year.as_str()).collect();
    assert_eq!(years, ["2020", "2021"], "按年升序（前端倒转为最新在前）");
    let names: Vec<&str> = result
        .by_account
        .iter()
        .map(|r| r.account_name.as_str())
        .collect();
    assert_eq!(names, ["账户A", "账户B"], "按账户名序");
}

#[test]
fn realized_pnl_summary_dividend_leg_follows_filters() {
    let conn = open();
    seed_account(&conn, "acc-a", "账户A", "investment", "CNY", 0);
    seed_account(&conn, "acc-b", "账户B", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-a", "501000", "基金A", "CNY", "unknown");
    seed_instrument(&conn, "inst-b", "502000", "基金B", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-a", "inst-a", 100_000, "CNY", "2021-03-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-b", "inst-b", 200_000, "CNY", "2021-04-01"),
    )
    .unwrap();

    let by_account = query_realized_pnl_summary(
        &conn,
        &PnlFilter {
            account_id: Some("acc-a".into()),
            instrument_id: None,
        },
    )
    .unwrap();
    assert_eq!(by_account.by_account.len(), 1);
    assert_eq!(by_account.by_account[0].account_name, "账户A");
    assert_eq!(by_account.by_account[0].dividend_cents, Some(100_000));
    assert_eq!(by_account.by_year.len(), 1);
    assert_eq!(by_account.by_year[0].dividend_cents, Some(100_000));

    let by_instrument = query_realized_pnl_summary(
        &conn,
        &PnlFilter {
            account_id: None,
            instrument_id: Some("inst-b".into()),
        },
    )
    .unwrap();
    assert_eq!(by_instrument.by_year.len(), 1);
    assert_eq!(by_instrument.by_year[0].dividend_cents, Some(200_000));
    assert_eq!(by_instrument.by_account.len(), 1);
    assert_eq!(by_instrument.by_account[0].account_name, "账户B");

    let none = query_realized_pnl_summary(
        &conn,
        &PnlFilter {
            account_id: Some("acc-a".into()),
            instrument_id: Some("inst-b".into()),
        },
    )
    .unwrap();
    assert!(none.by_year.is_empty());
    assert!(none.by_account.is_empty());
}

#[test]
fn realized_pnl_summary_dividend_leg_follows_soft_delete_and_hidden() {
    let conn = open();
    seed_account(&conn, "acc-live", "在用户", "investment", "CNY", 0);
    seed_account(&conn, "acc-del", "已删户", "investment", "CNY", 0);
    seed_account(&conn, "acc-hid", "隐藏户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dv", "501000", "基金A", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-live", "inst-dv", 100_000, "CNY", "2021-03-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-del", "inst-dv", 200_000, "CNY", "2021-03-02"),
    )
    .unwrap();
    let hidden_dividend = create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-hid", "inst-dv", 300_000, "CNY", "2021-03-03"),
    )
    .unwrap()
    .id;

    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-del'", [])
        .unwrap();
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hid'", [])
        .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_account.len(), 2);
    assert_eq!(
        result
            .by_account
            .iter()
            .map(|a| a.dividend_cents.unwrap_or(0))
            .sum::<i64>(),
        400_000
    );

    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        rusqlite::params![hidden_dividend],
    )
    .unwrap();
    let after = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(after.by_account.len(), 1);
    assert_eq!(after.by_account[0].dividend_cents, Some(100_000));
}

#[test]
fn realized_pnl_summary_counts_dividend_reinvestment_leg() {
    let conn = open();
    seed_account(&conn, "acc-drip", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-drip", "501000", "基金A", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-drip", "inst-drip", 123_180, "CNY", "2021-09-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-drip",
            "inst-drip",
            10.0,
            123_180,
            "2021-09-01",
        ),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_year.len(), 1);
    assert_row(&result.by_year[0], "2021", 0, 123_180, 123_180, "CNY");
}

#[test]
fn realized_pnl_summary_converted_dividends_of_two_currencies_merge_into_year_row() {
    let conn = open();
    seed_account(&conn, "acc-cny", "人民币户", "investment", "CNY", 0);
    seed_account(&conn, "acc-usd", "美元户", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2021-03-01"]);
    seed_instrument(&conn, "inst-cny", "501000", "基金A", "CNY", "unknown");
    seed_instrument(&conn, "inst-usd", "AAPL", "Apple", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-cny", "inst-cny", 100_000, "CNY", "2021-03-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-usd", "inst-usd", 250_000, "USD", "2021-03-01"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_year.len(), 1, "同一年不再按币种拆两行");
    assert_row(
        &result.by_year[0],
        "2021",
        0,
        100_000 + 250_000 * 7,
        100_000 + 250_000 * 7,
        "CNY",
    );
    assert_eq!(result.by_account.len(), 2);
}

#[test]
fn realized_pnl_summary_aggregates_multiple_accounts() {
    let conn = open();
    seed_account(&conn, "acc-a", "账户A", "investment", "USD", 0);
    seed_account(&conn, "acc-b", "账户B", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD", "unknown");

    create_transaction_internal(&conn, make_buy_input("acc-a", "inst-xyz", 10.0, 100_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-b", "inst-xyz", 5.0, 200_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-a", "inst-xyz", 4.0, 150_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-b", "inst-xyz", 2.0, 250_000, 0))
        .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.by_account.len(), 2);
    assert_eq!(result.by_account[0].account_id, "acc-a");
    assert_eq!(result.by_account[0].realized_pnl_cents, Some(2000));
    assert_eq!(result.by_account[1].account_id, "acc-b");
    assert_eq!(result.by_account[1].realized_pnl_cents, Some(1000));
}

#[test]
fn realized_pnl_summary_filter_by_account() {
    let conn = open();
    seed_account(&conn, "acc-a", "账户A", "investment", "USD", 0);
    seed_account(&conn, "acc-b", "账户B", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD", "unknown");

    create_transaction_internal(&conn, make_buy_input("acc-a", "inst-xyz", 10.0, 100_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-b", "inst-xyz", 5.0, 200_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-a", "inst-xyz", 4.0, 150_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-b", "inst-xyz", 2.0, 250_000, 0))
        .unwrap();

    let filter = PnlFilter {
        account_id: Some("acc-a".into()),
        instrument_id: None,
    };
    let result = query_realized_pnl_summary(&conn, &filter).unwrap();

    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].realized_gain_cents, Some(2000));
}

#[test]
fn realized_pnl_summary_excludes_soft_deleted_account() {
    let conn = open();
    seed_account(&conn, "acc-live", "在用户", "investment", "USD", 0);
    seed_account(&conn, "acc-del", "已删户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-sd", "SD", "Soft Del Corp", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-live", "inst-sd", 10.0, 100_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-live", "inst-sd", 4.0, 150_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_buy_input("acc-del", "inst-sd", 10.0, 100_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-del", "inst-sd", 4.0, 150_000, 0),
    )
    .unwrap();

    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-del'", [])
        .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-live");
    assert_eq!(result.by_account[0].account_name, "在用户");
    assert_eq!(result.by_account[0].realized_gain_cents, Some(2000));
}

#[test]
fn realized_pnl_summary_excludes_soft_deleted_sell() {
    let conn = open();
    seed_account(&conn, "acc-pnl", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-sd", "SD", "Soft Del Corp", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-pnl", "inst-sd", 10.0, 100_000, 0),
    )
    .unwrap();
    let sell_id = create_transaction_internal(
        &conn,
        make_sell_input("acc-pnl", "inst-sd", 4.0, 150_000, 0),
    )
    .unwrap()
    .id;

    let baseline = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(baseline.by_year[0].realized_gain_cents, Some(2000));

    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        rusqlite::params![sell_id],
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert!(result.by_year.is_empty());
    assert!(result.by_account.is_empty());
}

#[test]
fn realized_pnl_summary_filter_by_instrument() {
    let conn = open();
    seed_account(&conn, "acc-pnl", "美股", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-a", "AAPL", "Apple", "USD", "unknown");
    seed_instrument(&conn, "inst-b", "GOOGL", "Alphabet", "USD", "unknown");

    create_transaction_internal(&conn, make_buy_input("acc-pnl", "inst-a", 10.0, 100_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-pnl", "inst-b", 5.0, 200_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-pnl", "inst-a", 4.0, 150_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_sell_input("acc-pnl", "inst-b", 2.0, 250_000, 0))
        .unwrap();

    let filter = PnlFilter {
        account_id: None,
        instrument_id: Some("inst-a".into()),
    };
    let result = query_realized_pnl_summary(&conn, &filter).unwrap();

    assert_eq!(result.by_year.len(), 1);
    assert_eq!(result.by_year[0].realized_gain_cents, Some(2000));
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].realized_gain_cents, Some(2000));
}
