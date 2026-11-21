use rusqlite::Connection;

use crate::overview::query_investment_overview;
use crate::prices::{MarketPriceWrite, upsert_market_price};
use crate::{query_cumulative_pnl_summary, query_holdings_summary_by_currency};
use ledger_transaction::amount::convert_to_native_current;
use ledger_transaction::{TransactionInput, create_transaction_internal};
use tauri_app_lib::test_support::{
    open, seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_instrument,
};

use super::common::{make_buy_input, make_dividend_input, make_sell_input};

fn buy_in(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price_cents: i64,
    currency: &str,
    fee_cents: i64,
) -> TransactionInput {
    TransactionInput {
        currency_code: currency.into(),
        ..make_buy_input(account_id, instrument_id, qty, price_cents, fee_cents)
    }
}

fn seed_price(conn: &Connection, instrument_id: &str, price_cents: i64, currency: &str) {
    upsert_market_price(
        conn,
        &MarketPriceWrite {
            instrument_id,
            price_cents,
            currency_code: currency,
            priced_at: &ledger_infra::db::now_iso(),
            nav_date: None,
            source: Some("eastmoney"),
        },
    )
    .expect("现价缓存写入应成功");
}

fn cny_identity(conn: &Connection) {
    seed_exchange_rate(conn, "CNY", "CNY", 1.0);
}

#[test]
fn overview_equals_two_legs_sum_binding_existing_single_point() {
    let conn = open();
    seed_account(&conn, "acc-inv", "投资账户", "investment", "CNY", 200_000);
    seed_account(&conn, "acc-life", "生活现金", "cash", "CNY", 999_999);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_instrument(&conn, "inst-a", "IA", "标的A", "CNY", "sh");
    cny_identity(&conn);
    create_transaction_internal(
        &conn,
        buy_in("acc-inv", "inst-a", 10.0, 1_000_000, "CNY", 0),
    )
    .unwrap();
    seed_price(&conn, "inst-a", 1_200_000, "CNY");

    let overview = query_investment_overview(&conn).unwrap();
    assert_eq!(overview.native_currency, "CNY");
    assert_eq!(
        overview.investment_cash_cents, 100_000,
        "生活现金不计入现金腿"
    );
    assert_eq!(overview.holdings_market_value_cents, 120_000);
    assert_eq!(
        overview.investable_assets_cents,
        overview.investment_cash_cents + overview.holdings_market_value_cents,
        "合计 = 两腿之和"
    );
    assert_eq!(
        overview.investable_assets_cents,
        super::super::query_investable_assets_cents(&conn).unwrap(),
        "拆分后与既有可投资资产单点恒等（财务自由度分子 / 跨账本汇总同源）"
    );
    assert_eq!(overview.missing_price_holding_count, 0);
    assert!(overview.has_investment_account);
}

#[test]
fn overview_folds_cash_leg_by_account_currency() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 100_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
        &["2026-01-10", "2026-01-20", "2026-02-10"],
    );

    let overview = query_investment_overview(&conn).unwrap();
    assert_eq!(overview.native_currency, "CNY", "折算目标 = 全局默认币种");
    assert_eq!(
        overview.investment_cash_cents, 700_000,
        "1000 美元现金折 7.0 → 7000 元"
    );
    assert_eq!(overview.holdings_market_value_cents, 0);
    assert_eq!(overview.investable_assets_cents, 700_000);
}

#[test]
fn overview_folds_holdings_leg_by_account_currency() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股券商", "investment", "USD", 244_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_instrument(&conn, "inst-nvda", "NVDA", "英伟达", "USD", "nasdaq");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
        &["2026-01-10", "2026-01-20", "2026-02-10"],
    );
    create_transaction_internal(
        &conn,
        buy_in("acc-usd", "inst-nvda", 2.0, 1_000_000, "USD", 0),
    )
    .unwrap();
    seed_price(&conn, "inst-nvda", 1_500_000, "USD");

    let overview = query_investment_overview(&conn).unwrap();
    assert_eq!(
        overview.holdings_market_value_cents, 210_000,
        "2 × 150 美元 × 7.0 → 2100 元"
    );
    assert_eq!(
        overview.investment_cash_cents, 1_568_000,
        "账户现金 224000 美分（USD）× 7.0"
    );
    assert_eq!(
        overview.investable_assets_cents,
        overview.investment_cash_cents + overview.holdings_market_value_cents
    );
    assert_eq!(
        overview.investable_assets_cents,
        super::super::query_investable_assets_cents(&conn).unwrap(),
        "恒等绑定：两腿之和 = 既有可投资资产单点"
    );
}

#[test]
fn overview_missing_rate_raises_coded_error() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 80_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();

    let err = query_investment_overview(&conn).unwrap_err();
    assert!(
        err.is_code("fx.rate-missing"),
        "缺汇率应报 fx.rate-missing，实际 {err:?}"
    );
}

#[test]
fn overview_excludes_hidden_accounts_and_counts_unpriced_holdings() {
    let conn = open();
    seed_account(&conn, "acc-vis", "可见账户", "investment", "CNY", 50_000);
    seed_account(&conn, "acc-h", "隐藏账户", "investment", "CNY", 70_000);
    seed_instrument(&conn, "inst-priced", "PRICED", "有价标的", "CNY", "sh");
    seed_instrument(&conn, "inst-bare", "BARE", "缺价标的", "CNY", "sh");
    seed_instrument(&conn, "inst-h", "HID", "隐藏标的", "CNY", "sh");
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    cny_identity(&conn);
    create_transaction_internal(
        &conn,
        buy_in("acc-vis", "inst-priced", 1.0, 1_000_000, "CNY", 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_in("acc-vis", "inst-bare", 3.0, 1_000_000, "CNY", 0),
    )
    .unwrap();
    create_transaction_internal(&conn, buy_in("acc-h", "inst-h", 5.0, 1_000_000, "CNY", 0))
        .unwrap();
    seed_price(&conn, "inst-priced", 1_100_000, "CNY");
    seed_price(&conn, "inst-h", 1_000_000, "CNY");
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-h'", [])
        .unwrap();

    let overview = query_investment_overview(&conn).unwrap();
    assert_eq!(
        overview.investment_cash_cents, 10_000,
        "隐藏账户现金不进现金腿"
    );
    assert_eq!(
        overview.holdings_market_value_cents, 11_000,
        "隐藏账户持仓不进持仓腿；缺价持仓不以零计入"
    );
    assert_eq!(
        overview.missing_price_holding_count, 1,
        "只数可见账户的未计入持仓"
    );
    assert!(overview.has_investment_account);
}

#[test]
fn overview_without_investment_account_reports_zero_with_guide_fact() {
    let conn = open();
    seed_account(&conn, "acc-life", "生活现金", "cash", "CNY", 999_999);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();

    let overview = query_investment_overview(&conn).unwrap();
    assert_eq!(overview.native_currency, "CNY");
    assert_eq!(overview.investable_assets_cents, 0);
    assert_eq!(overview.investment_cash_cents, 0);
    assert_eq!(overview.holdings_market_value_cents, 0);
    assert_eq!(overview.missing_price_holding_count, 0);
    assert!(!overview.has_investment_account);
}

#[test]
fn overview_totals_fold_to_native_currency() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-x", "XX", "标的X", "USD", "nasdaq");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
        &["2026-01-10", "2026-01-20", "2026-02-10"],
    );
    create_transaction_internal(&conn, buy_in("acc-usd", "inst-x", 2.0, 1_000_000, "USD", 0))
        .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            ..make_sell_input("acc-usd", "inst-x", 1.0, 1_200_000, 0)
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            ..make_dividend_input("acc-usd", "inst-x", 3_000, "USD")
        },
    )
    .unwrap();
    seed_price(&conn, "inst-x", 1_500_000, "USD");

    let overview = query_investment_overview(&conn).unwrap();
    assert_eq!(overview.native_currency, "CNY");
    assert_eq!(overview.total_market_value_cents, 105_000, "$150 × 7.0");
    assert_eq!(overview.unrealized_pnl_cents, 35_000, "($150−$100) × 7.0");
    assert_eq!(
        overview.cumulative_pnl_cents, 70_000,
        "($50 未实现 + $20 已实现 + $30 分红) × 7.0"
    );
    assert_eq!(
        overview.total_market_value_cents,
        overview.holdings_market_value_cents
    );

    let grouped_market_value: i64 = query_holdings_summary_by_currency(&conn)
        .unwrap()
        .iter()
        .map(|g| {
            convert_to_native_current(&conn, g.market_value_cents.unwrap(), &g.currency_code)
                .unwrap()
        })
        .sum();
    assert_eq!(grouped_market_value, overview.total_market_value_cents);
    let grouped_cumulative: i64 = query_cumulative_pnl_summary(&conn)
        .unwrap()
        .iter()
        .map(|g| {
            convert_to_native_current(&conn, g.cumulative_pnl_cents, &g.currency_code).unwrap()
        })
        .sum();
    assert_eq!(grouped_cumulative, overview.cumulative_pnl_cents);
}

#[test]
fn overview_totals_exclude_hidden_accounts() {
    let conn = open();
    seed_account(&conn, "acc-vis", "可见账户", "investment", "CNY", 50_000);
    seed_instrument(&conn, "inst-vis", "VIS", "可见标的", "CNY", "sh");
    seed_instrument(&conn, "inst-bare", "BARE", "缺价标的", "CNY", "sh");
    seed_account(&conn, "acc-h", "隐藏账户", "investment", "CNY", 70_000);
    seed_instrument(&conn, "inst-h", "HID", "隐藏标的", "CNY", "sh");
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    cny_identity(&conn);
    create_transaction_internal(
        &conn,
        buy_in("acc-vis", "inst-vis", 1.0, 1_000_000, "CNY", 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_in("acc-vis", "inst-bare", 3.0, 1_000_000, "CNY", 0),
    )
    .unwrap();
    create_transaction_internal(&conn, buy_in("acc-h", "inst-h", 2.0, 1_000_000, "CNY", 0))
        .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            ..make_sell_input("acc-h", "inst-h", 1.0, 900_000, 0)
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            ..make_dividend_input("acc-h", "inst-h", 5_000, "CNY")
        },
    )
    .unwrap();
    seed_price(&conn, "inst-vis", 1_100_000, "CNY");
    seed_price(&conn, "inst-h", 1_200_000, "CNY");
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-h'", [])
        .unwrap();

    let overview = query_investment_overview(&conn).unwrap();
    assert_eq!(
        overview.total_market_value_cents, 11_000,
        "只计可见账户持仓市值；缺价标的不以零计入"
    );
    assert_eq!(overview.unrealized_pnl_cents, 1_000);
    assert_eq!(
        overview.cumulative_pnl_cents, 1_000,
        "隐藏账户的已实现 −¥10 与分红 ¥50 一并不进累计收益"
    );
    assert_eq!(overview.missing_price_holding_count, 1);

    let grouped_market_value: i64 = query_holdings_summary_by_currency(&conn)
        .unwrap()
        .iter()
        .map(|g| {
            convert_to_native_current(&conn, g.market_value_cents.unwrap(), &g.currency_code)
                .unwrap()
        })
        .sum();
    assert_eq!(grouped_market_value, 23_000);
    let grouped_cumulative: i64 = query_cumulative_pnl_summary(&conn)
        .unwrap()
        .iter()
        .map(|g| {
            convert_to_native_current(&conn, g.cumulative_pnl_cents, &g.currency_code).unwrap()
        })
        .sum();
    assert_eq!(grouped_cumulative, 7_000);
}

#[test]
fn overview_totals_missing_rate_raises_coded_error() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-y", "YY", "标的Y", "USD", "nasdaq");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
        &["2026-01-10", "2026-01-20", "2026-02-10"],
    );
    create_transaction_internal(&conn, buy_in("acc-usd", "inst-y", 1.0, 1_000_000, "USD", 0))
        .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            ..make_sell_input("acc-usd", "inst-y", 1.0, 1_100_000, 0)
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            ..make_dividend_input("acc-usd", "inst-y", 500, "USD")
        },
    )
    .unwrap();
    conn.execute("DELETE FROM exchange_rates", []).unwrap();

    let err = query_investment_overview(&conn).unwrap_err();
    assert!(
        err.is_code("fx.rate-missing"),
        "缺汇率应报 fx.rate-missing，实际 {err:?}"
    );
}
