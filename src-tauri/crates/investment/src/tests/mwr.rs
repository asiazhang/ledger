use chrono::NaiveDate;
use ledger_transaction::{
    SecurityOrigin, TransactionInput, create_transaction_internal, delete_transaction_internal,
};

use super::super::mwr::xirr;
use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{
    open, seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_fx_rate_history,
    seed_instrument, seed_market_price, seed_price_history,
};

const TODAY: &str = "2027-01-01";

fn today() -> NaiveDate {
    NaiveDate::parse_from_str(TODAY, "%Y-%m-%d").unwrap()
}

fn buy_on(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        date: date.into(),
        ..make_buy_input(account_id, instrument_id, qty, price, fee)
    }
}

fn sell_on(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        date: date.into(),
        ..make_sell_input(account_id, instrument_id, qty, price, fee)
    }
}

fn opening_buy_on(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        date: date.into(),
        origin: Some(SecurityOrigin::Opening),
        ..make_buy_input(account_id, instrument_id, qty, price, fee)
    }
}

fn dividend_on(
    account_id: &str,
    instrument_id: &str,
    amount_cents: i64,
    date: &str,
) -> TransactionInput {
    let mut input = make_dividend_input(account_id, instrument_id, amount_cents, "CNY");
    input.date = date.into();
    input
}

fn mwr_on(conn: &rusqlite::Connection, range: &MwrRange) -> MoneyWeightedReturnSummary {
    mwr::query_money_weighted_return_summary_on(conn, range, today()).unwrap()
}

fn instrument_rate(
    summary: &MoneyWeightedReturnSummary,
    account_id: &str,
    instrument_id: &str,
) -> Option<f64> {
    summary
        .by_instrument
        .iter()
        .find(|r| r.account_id == account_id && r.instrument_id == instrument_id)
        .and_then(|r| r.rate)
}

fn account_rate(summary: &MoneyWeightedReturnSummary, account_id: &str) -> Option<f64> {
    summary
        .by_account
        .iter()
        .find(|r| r.account_id == account_id)
        .and_then(|r| r.rate)
}

fn total_rate(summary: &MoneyWeightedReturnSummary, currency: &str) -> Option<f64> {
    summary
        .total
        .iter()
        .find(|r| r.currency_code == currency)
        .and_then(|r| r.rate)
}

fn account_basis(summary: &MoneyWeightedReturnSummary, account_id: &str) -> Option<MwrBasis> {
    summary
        .by_account
        .iter()
        .find(|r| r.account_id == account_id)
        .map(|r| r.basis)
}

fn total_basis(summary: &MoneyWeightedReturnSummary, currency: &str) -> Option<MwrBasis> {
    summary
        .total
        .iter()
        .find(|r| r.currency_code == currency)
        .map(|r| r.basis)
}

fn instrument_basis(
    summary: &MoneyWeightedReturnSummary,
    account_id: &str,
    instrument_id: &str,
) -> Option<MwrBasis> {
    summary
        .by_instrument
        .iter()
        .find(|r| r.account_id == account_id && r.instrument_id == instrument_id)
        .map(|r| r.basis)
}

fn assert_close(actual: Option<f64>, expected: f64) {
    let rate = actual.unwrap_or_else(|| panic!("预期有收益率，实际为 None（预期 {expected}）"));
    assert!(
        (rate - expected).abs() < 1e-9,
        "XIRR {rate} 与手算值 {expected} 偏差超过尾差口径 1e-9"
    );
}

type XirrCase = (&'static str, Vec<(f64, f64)>, f64);

#[test]
fn xirr_matches_hand_computed_samples() {
    let cases: &[XirrCase] = &[
        ("单笔买入持有", vec![(0.0, -10000.0), (1.0, 11000.0)], 0.1),
        ("亏损年", vec![(0.0, -10000.0), (1.0, 9000.0)], -0.1),
        (
            "定投",
            vec![(0.0, -5000.0), (181.0 / 365.0, -5000.0), (1.0, 10600.0)],
            0.080_296_779_864_777_48,
        ),
        (
            "部分卖出",
            vec![(0.0, -10000.0), (200.0 / 365.0, 4000.0), (1.0, 7000.0)],
            0.121_236_342_832_381_08,
        ),
        (
            "分红",
            vec![(0.0, -10000.0), (100.0 / 365.0, 300.0), (1.0, 11200.0)],
            0.153_272_508_100_482_65,
        ),
        (
            "零金额流",
            vec![(0.0, -10000.0), (0.5, 0.0), (1.0, 11000.0)],
            0.1,
        ),
    ];
    for (name, flows, expected) in cases {
        let rate = xirr(flows).unwrap_or_else(|| panic!("{name}: 预期有解 {expected}，实际 None"));
        assert!(
            (rate - expected).abs() < 1e-9,
            "{name}: XIRR {rate} 与手算值 {expected} 偏差超过尾差口径 1e-9"
        );
    }
}

#[test]
fn xirr_returns_none_for_unsolvable_or_degenerate_flows() {
    assert_eq!(xirr(&[(0.0, -100.0)]), None);
    assert_eq!(xirr(&[(0.0, 100.0), (1.0, 50.0)]), None);
    assert_eq!(xirr(&[(0.0, -100.0), (1.0, -50.0)]), None);
    assert_eq!(xirr(&[]), None);
    assert_eq!(xirr(&[(0.0, -100.0), (0.0, 100.0)]), None);
}

#[test]
fn mwr_single_buy_and_hold_matches_hand_computed_rate() {
    let conn = open();
    seed_account(&conn, "acc-m1", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-m1", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-m1", "inst-m1", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-m1", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(instrument_rate(&summary, "acc-m1", "inst-m1"), 0.1);
    assert_close(account_rate(&summary, "acc-m1"), 0.1);
    assert_close(total_rate(&summary, "CNY"), 0.1);
    assert_eq!(summary.by_instrument[0].currency_code, "CNY");
    assert_eq!(summary.by_instrument[0].basis, MwrBasis::Annualized);
    assert_eq!(
        account_basis(&summary, "acc-m1"),
        Some(MwrBasis::Annualized)
    );
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Annualized));
}

#[test]
fn mwr_buy_fee_is_part_of_negative_flow() {
    let conn = open();
    seed_account(&conn, "acc-fee", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-fee", "600000", "浦发银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-fee", "inst-fee", 10.0, 100_000, 100, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-fee", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(account_rate(&summary, "acc-fee"), 11000.0 / 10100.0 - 1.0);
}

#[test]
fn mwr_dca_multiple_buys_matches_hand_computed_rate() {
    let conn = open();
    seed_account(&conn, "acc-dca", "定投户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dca", "000001", "平安银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-dca", "inst-dca", 5.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-dca", "inst-dca", 5.0, 100_000, 0, "2026-07-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-dca", 106_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(account_rate(&summary, "acc-dca"), 0.080_296_779_864_777_48);
}

#[test]
fn mwr_partial_sell_net_proceeds_are_positive_flow() {
    let conn = open();
    seed_account(&conn, "acc-ps", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ps", "600036", "招商银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-ps", "inst-ps", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        sell_on("acc-ps", "inst-ps", 4.0, 105_000, 200, "2026-07-20"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-ps", 116_667, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(account_rate(&summary, "acc-ps"), 0.121_236_342_832_381_08);
}

#[test]
fn mwr_dividend_is_positive_flow() {
    let conn = open();
    seed_account(&conn, "acc-div", "基金户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-div", "000001", "华夏成长", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-div", "inst-div", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(&conn, dividend_on("acc-div", "inst-div", 300, "2026-04-11"))
        .unwrap();
    seed_market_price(&conn, "inst-div", 112_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(account_rate(&summary, "acc-div"), 0.153_272_508_100_482_65);
}

#[test]
fn mwr_convert_splits_into_two_legs_at_instrument_granularity() {
    let conn = open();
    seed_account(&conn, "acc-cv", "转换户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-cv-a", "000001", "转出基金", "CNY", "unknown");
    seed_instrument(&conn, "inst-cv-b", "000002", "转入基金", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-cv", "inst-cv-a", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            date: "2026-04-11".into(),
            ..make_convert_input("acc-cv", "inst-cv-a", "inst-cv-b", 5.0, 5.0, 4500, 4500, 0)
        },
    )
    .unwrap();
    seed_market_price(&conn, "inst-cv-a", 110_000, "CNY");
    seed_market_price(&conn, "inst-cv-b", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(
        instrument_rate(&summary, "acc-cv", "inst-cv-a"),
        0.078_034_331_116_894_84,
    );
    assert_close(
        instrument_rate(&summary, "acc-cv", "inst-cv-b"),
        0.140_282_781_268_959_96,
    );
    assert_close(account_rate(&summary, "acc-cv"), 0.1);
    assert_close(total_rate(&summary, "CNY"), 0.1);
}

#[test]
fn mwr_split_is_zero_cash_flow() {
    let conn = open();
    seed_account(&conn, "acc-sp", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-sp", "600000", "浦发银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-sp", "inst-sp", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 20.0)).unwrap();
    seed_market_price(&conn, "inst-sp", 100_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(account_rate(&summary, "acc-sp"), 2.0);
}

#[test]
fn mwr_skips_unvalued_holding_from_row_and_aggregates() {
    let conn = open();
    seed_account(&conn, "acc-np", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-np-1", "600519", "贵州茅台", "CNY", "unknown");
    seed_instrument(&conn, "inst-np-2", "000001", "平安银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-np", "inst-np-1", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-np", "inst-np-2", 5.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-np-1", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(instrument_rate(&summary, "acc-np", "inst-np-2"), None);
    assert_close(account_rate(&summary, "acc-np"), 0.1);
    assert_close(total_rate(&summary, "CNY"), 0.1);
}

#[test]
fn mwr_groups_ledger_total_by_currency_without_mixing() {
    let conn = open();
    seed_account(&conn, "acc-us", "美股户", "investment", "USD", 0);
    seed_account(&conn, "acc-cn", "A 股户", "investment", "CNY", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 1.0, &["2026-01-01"]);
    seed_instrument(&conn, "inst-us", "AAPL", "Apple", "USD", "unknown");
    seed_instrument(&conn, "inst-cn", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-us", "inst-us", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-cn", "inst-cn", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-us", 110_000, "USD");
    seed_market_price(&conn, "inst-cn", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(summary.total.len(), 2);
    assert_close(total_rate(&summary, "USD"), 0.1);
    assert_close(total_rate(&summary, "CNY"), 0.2);
    assert_close(account_rate(&summary, "acc-us"), 0.1);
    assert_close(account_rate(&summary, "acc-cn"), 0.2);
}

#[test]
fn mwr_reports_none_when_flows_have_no_solution() {
    let conn = open();
    seed_account(&conn, "acc-ns", "基金户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ns", "000001", "华夏成长", "CNY", "unknown");
    create_transaction_internal(&conn, dividend_on("acc-ns", "inst-ns", 300, "2026-04-11"))
        .unwrap();

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(instrument_rate(&summary, "acc-ns", "inst-ns"), None);
    let account = summary
        .by_account
        .iter()
        .find(|r| r.account_id == "acc-ns")
        .expect("有现金流的账户应有账户级行");
    assert_eq!(account.rate, None);
    let total = summary
        .total
        .iter()
        .find(|r| r.currency_code == "CNY")
        .expect("全账级应有 CNY 组");
    assert_eq!(total.rate, None);
}

#[test]
fn mwr_drip_legs_cancel_within_account() {
    let conn = open();
    seed_account(&conn, "acc-dr", "基金户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dr", "000001", "华夏成长", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-dr", "inst-dr", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(&conn, dividend_on("acc-dr", "inst-dr", 500, "2026-04-11"))
        .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-dr", "inst-dr", 0.5, 100_000, 0, "2026-04-11"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-dr", 100_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(account_rate(&summary, "acc-dr"), 0.05);
    assert_close(instrument_rate(&summary, "acc-dr", "inst-dr"), 0.05);
    assert_close(total_rate(&summary, "CNY"), 0.05);
}

#[test]
fn mwr_range_folds_starting_position_at_start_date_market_value() {
    let conn = open();
    seed_account(&conn, "acc-rg", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-rg", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-rg", "inst-rg", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-rg-1", "inst-rg", "2026-01-05", 100_000, "CNY");
    seed_price_history(&conn, "ph-rg-2", "inst-rg", "2026-07-01", 110_000, "CNY");
    seed_price_history(&conn, "ph-rg-3", "inst-rg", "2026-12-25", 120_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-07-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_close(account_rate(&summary, "acc-rg"), 0.188_395_514_532_513_57);
    assert_close(
        instrument_rate(&summary, "acc-rg", "inst-rg"),
        0.188_395_514_532_513_57,
    );
}

#[test]
fn mwr_range_converts_boundary_value_with_period_fx_rate() {
    let conn = open();
    seed_account(&conn, "acc-fx", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-fx", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-fx", "inst-fx", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-fx-1", "inst-fx", "2026-07-01", 100_000, "USD");
    seed_fx_rate_history(&conn, "fxh-fx-1", "USD", "CNY", "2026-07-01", 7.0);
    seed_price_history(&conn, "ph-fx-2", "inst-fx", "2026-12-25", 110_000, "USD");
    seed_fx_rate_history(&conn, "fxh-fx-2", "USD", "CNY", "2026-12-25", 7.0);

    let range = MwrRange {
        start_date: Some("2026-07-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_close(account_rate(&summary, "acc-fx"), 0.208_121_156_121_199_33);
    assert_eq!(summary.total[0].currency_code, "CNY");
}

#[test]
fn mwr_range_includes_start_day_dividend() {
    let conn = open();
    seed_account(&conn, "acc-dd", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-dd", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-dd", "inst-dd", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(&conn, dividend_on("acc-dd", "inst-dd", 500, "2026-07-01"))
        .unwrap();
    seed_price_history(&conn, "ph-dd-1", "inst-dd", "2026-07-01", 100_000, "CNY");
    seed_market_price(&conn, "inst-dd", 100_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-07-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_close(account_rate(&summary, "acc-dd"), 0.107_106_976_057_223_55);
}

#[test]
fn mwr_rejects_invalid_range() {
    let conn = open();
    let bad_format = MwrRange {
        start_date: Some("2026/07/01".into()),
        end_date: None,
    };
    let err = mwr::query_money_weighted_return_summary_on(&conn, &bad_format, today()).unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));

    seed_account(&conn, "acc-vd", "A 股户", "investment", "CNY", 0);
    let reversed = MwrRange {
        start_date: Some("2027-01-01".into()),
        end_date: Some("2026-07-01".into()),
    };
    let err = mwr::query_money_weighted_return_summary_on(&conn, &reversed, today()).unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
}

#[test]
fn mwr_excludes_soft_deleted_accounts_and_transactions() {
    let conn = open();
    seed_account(&conn, "acc-sd", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-sd", "600519", "贵州茅台", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        buy_on("acc-sd", "inst-sd", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-sd", 110_000, "CNY");
    let dropped = create_transaction_internal(
        &conn,
        buy_on("acc-sd", "inst-sd", 5.0, 110_000, 0, "2026-06-01"),
    )
    .unwrap()
    .id;
    delete_transaction_internal(&conn, &dropped).unwrap();

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(account_rate(&summary, "acc-sd"), 0.1);
}

#[test]
fn mwr_opening_balance_reports_cumulative_rate_instead_of_annualized() {
    let conn = open();
    seed_account(&conn, "acc-op", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-op",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-op", "inst-op", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-op", 110_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        instrument_basis(&summary, "acc-op", "inst-op"),
        Some(MwrBasis::Cumulative),
        "含期初存量的标的应标未年化口径"
    );
    assert_close(instrument_rate(&summary, "acc-op", "inst-op"), 0.1);
    assert_eq!(
        account_basis(&summary, "acc-op"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-op"), 0.1);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 0.1);
}

#[test]
fn mwr_opening_balance_with_real_flows_restores_annualized_from_first_real_flow() {
    let conn = open();
    seed_account(&conn, "acc-ob", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-ob",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-ob", "inst-ob", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-ob", "inst-ob", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-ob-1", "inst-ob", "2026-09-01", 110_000, "CNY");
    seed_market_price(&conn, "inst-ob", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        instrument_basis(&summary, "acc-ob", "inst-ob"),
        Some(MwrBasis::Annualized),
        "含期初存量且有真实流水的标的应恢复年化口径"
    );
    assert_close(
        instrument_rate(&summary, "acc-ob", "inst-ob"),
        0.297_346_368_102_668_93,
    );
    assert_close(account_rate(&summary, "acc-ob"), 0.297_346_368_102_668_93);
    assert_close(total_rate(&summary, "CNY"), 0.297_346_368_102_668_93);
}

#[test]
fn mwr_opening_balance_with_real_flows_ignores_range_selection() {
    let conn = open();
    seed_account(&conn, "acc-or", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-or",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-or", "inst-or", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-or", "inst-or", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-or-1", "inst-or", "2026-09-01", 110_000, "CNY");
    seed_market_price(&conn, "inst-or", 120_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-10-01".into()),
        end_date: Some("2026-12-31".into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_eq!(
        instrument_basis(&summary, "acc-or", "inst-or"),
        Some(MwrBasis::Annualized)
    );
    assert_close(
        instrument_rate(&summary, "acc-or", "inst-or"),
        0.297_346_368_102_668_93,
    );
}

#[test]
fn mwr_opening_balance_with_real_flows_skips_when_fold_price_missing() {
    let conn = open();
    seed_account(&conn, "acc-np", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-np",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-np", "inst-np", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-np", "inst-np", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-np-1", "inst-np", "2026-10-01", 110_000, "CNY");
    seed_market_price(&conn, "inst-np", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(instrument_rate(&summary, "acc-np", "inst-np"), None);
    assert_eq!(account_rate(&summary, "acc-np"), None);
    assert_eq!(total_rate(&summary, "CNY"), None);
}

#[test]
fn mwr_opening_balance_with_real_flows_folds_at_period_fx_rate() {
    let conn = open();
    seed_account(&conn, "acc-of", "雪球基金", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-of", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-of", "inst-of", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-of", "inst-of", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-of-1", "inst-of", "2026-09-01", 100_000, "USD");
    seed_fx_rate_history(&conn, "fxh-of-1", "USD", "CNY", "2026-09-01", 7.0);
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_market_price(&conn, "inst-of", 110_000, "USD");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        instrument_basis(&summary, "acc-of", "inst-of"),
        Some(MwrBasis::Annualized)
    );
    assert_close(
        instrument_rate(&summary, "acc-of", "inst-of"),
        0.329_960_587_626_393_8,
    );
}

#[test]
fn mwr_opening_balance_with_real_flows_skips_when_fold_fx_missing() {
    let conn = open();
    seed_account(&conn, "acc-nf", "雪球基金", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-nf", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-nf", "inst-nf", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-nf", "inst-nf", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-nf-1", "inst-nf", "2026-09-01", 100_000, "USD");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_market_price(&conn, "inst-nf", 110_000, "USD");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(instrument_rate(&summary, "acc-nf", "inst-nf"), None);
    assert_eq!(account_rate(&summary, "acc-nf"), None);
    assert_eq!(total_rate(&summary, "CNY"), None);
}

#[test]
fn mwr_mixed_account_merges_restored_flows_with_regular_pairs() {
    let conn = open();
    seed_account(&conn, "acc-mx", "雪球基金", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-mr", "600519", "贵州茅台", "CNY", "unknown");
    seed_instrument(
        &conn,
        "inst-mo",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        buy_on("acc-mx", "inst-mr", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-mr", 110_000, "CNY");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-mx", "inst-mo", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-mx", "inst-mo", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-mo-1", "inst-mo", "2026-09-01", 110_000, "CNY");
    seed_market_price(&conn, "inst-mo", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_close(instrument_rate(&summary, "acc-mx", "inst-mr"), 0.1);
    assert_close(
        instrument_rate(&summary, "acc-mx", "inst-mo"),
        0.297_346_368_102_668_93,
    );
    assert_close(account_rate(&summary, "acc-mx"), 0.164_056_601_667_387_7);
    assert_close(total_rate(&summary, "CNY"), 0.164_056_601_667_387_7);
}

#[test]
fn mwr_opening_balance_ignores_range_boundaries() {
    let conn = open();
    seed_account(&conn, "acc-or", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-or",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-or", "inst-or", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-or", 110_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-09-01".into()),
        end_date: Some("2026-12-31".into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_eq!(
        instrument_basis(&summary, "acc-or", "inst-or"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(instrument_rate(&summary, "acc-or", "inst-or"), 0.1);
}

#[test]
fn mwr_origin_is_rejected_on_non_buy_kinds() {
    let conn = open();
    seed_account(&conn, "acc-og", "A 股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-og", "600519", "贵州茅台", "CNY", "unknown");
    let mut input = sell_on("acc-og", "inst-og", 1.0, 100_000, 0, "2026-01-01");
    input.origin = Some(SecurityOrigin::Opening);

    let error = create_transaction_internal(&conn, input).unwrap_err();
    assert!(
        error.to_string().contains("不能携带证券来源口径"),
        "卖出携带 origin 应被准入守卫拒绝，实际: {error}"
    );
}

#[test]
fn mwr_collection_with_opening_only_pair_degrades_despite_restored_pair() {
    let conn = open();
    seed_account(&conn, "acc-ix", "雪球基金", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ix-a", "003155", "仅存量标的", "CNY", "unknown");
    seed_instrument(
        &conn,
        "inst-ix-b",
        "003156",
        "存量再买标的",
        "CNY",
        "unknown",
    );
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-ix", "inst-ix-a", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-ix-a", 110_000, "CNY");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-ix", "inst-ix-b", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-ix", "inst-ix-b", 5.0, 100_000, 0, "2026-09-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-ix-1", "inst-ix-b", "2026-09-01", 100_000, "CNY");
    seed_market_price(&conn, "inst-ix-b", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        instrument_basis(&summary, "acc-ix", "inst-ix-a"),
        Some(MwrBasis::Cumulative)
    );
    assert_eq!(
        instrument_basis(&summary, "acc-ix", "inst-ix-b"),
        Some(MwrBasis::Annualized)
    );
    assert_eq!(
        account_basis(&summary, "acc-ix"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-ix"), 0.16);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 0.16);
}

#[test]
fn mwr_mixed_collection_degrades_whole_item_to_cumulative() {
    let conn = open();
    seed_account(&conn, "acc-mx", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-mx-op",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    seed_instrument(&conn, "inst-mx-tr", "000001", "平安银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-mx", "inst-mx-op", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-mx", "inst-mx-tr", 5.0, 100_000, 0, "2026-08-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-mx-op", 110_000, "CNY");
    seed_market_price(&conn, "inst-mx-tr", 108_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        instrument_basis(&summary, "acc-mx", "inst-mx-op"),
        Some(MwrBasis::Cumulative)
    );
    assert_eq!(
        instrument_basis(&summary, "acc-mx", "inst-mx-tr"),
        Some(MwrBasis::Annualized)
    );
    assert_eq!(
        account_basis(&summary, "acc-mx"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-mx"), 1400.0 / 15000.0);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 1400.0 / 15000.0);
}

#[test]
fn mwr_currency_groups_flip_basis_independently() {
    let conn = open();
    seed_account(&conn, "acc-oc", "雪球基金", "investment", "CNY", 0);
    seed_account(&conn, "acc-uc", "美股户", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 1.0, &["2026-01-01"]);
    seed_instrument(
        &conn,
        "inst-oc",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    seed_instrument(&conn, "inst-uc", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-oc", "inst-oc", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-uc", "inst-uc", 10.0, 100_000, 0, "2026-01-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-oc", 110_000, "CNY");
    seed_market_price(&conn, "inst-uc", 120_000, "USD");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        account_basis(&summary, "acc-oc"),
        Some(MwrBasis::Cumulative)
    );
    assert_eq!(
        account_basis(&summary, "acc-uc"),
        Some(MwrBasis::Annualized)
    );
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_eq!(total_basis(&summary, "USD"), Some(MwrBasis::Annualized));
    assert_close(account_rate(&summary, "acc-oc"), 0.1);
    assert_close(account_rate(&summary, "acc-uc"), 0.2);
    assert_close(total_rate(&summary, "CNY"), 0.1);
    assert_close(total_rate(&summary, "USD"), 0.2);
}

#[test]
fn mwr_mixed_collection_range_keeps_cumulative_with_lifetime_opening() {
    let conn = open();
    seed_account(&conn, "acc-mr", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-mr-op",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    seed_instrument(&conn, "inst-mr-tr", "000001", "平安银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-mr", "inst-mr-op", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-mr", "inst-mr-tr", 5.0, 100_000, 0, "2026-08-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-mr-1", "inst-mr-tr", "2026-08-15", 100_000, "CNY");
    seed_market_price(&conn, "inst-mr-op", 110_000, "CNY");
    seed_market_price(&conn, "inst-mr-tr", 100_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-09-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_eq!(
        account_basis(&summary, "acc-mr"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-mr"), 1000.0 / 15000.0);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 1000.0 / 15000.0);
}

#[test]
fn mwr_degraded_collection_excludes_unvalued_pairs_from_aggregate() {
    let conn = open();
    seed_account(&conn, "acc-du", "雪球基金", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        "inst-du-op",
        "003156",
        "招商招悦纯债A",
        "CNY",
        "unknown",
    );
    seed_instrument(&conn, "inst-du-tr", "000001", "平安银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-du-np", "600000", "浦发银行", "CNY", "unknown");
    create_transaction_internal(
        &conn,
        opening_buy_on("acc-du", "inst-du-op", 10.0, 100_000, 0, "2026-06-30"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-du", "inst-du-tr", 5.0, 100_000, 0, "2026-08-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        buy_on("acc-du", "inst-du-np", 8.0, 100_000, 0, "2026-08-01"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-du-op", 110_000, "CNY");
    seed_market_price(&conn, "inst-du-tr", 120_000, "CNY");

    let summary = mwr_on(&conn, &MwrRange::default());
    assert_eq!(
        account_basis(&summary, "acc-du"),
        Some(MwrBasis::Cumulative)
    );
    assert_close(account_rate(&summary, "acc-du"), 2000.0 / 15000.0);
    assert_eq!(total_basis(&summary, "CNY"), Some(MwrBasis::Cumulative));
    assert_close(total_rate(&summary, "CNY"), 2000.0 / 15000.0);
}

#[test]
fn mwr_boundary_value_uses_constant_price_not_flat_history_rows() {
    let conn = open();
    seed_account(&conn, "acc-const", "货基户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = 'inst-const'",
        [],
    )
    .unwrap();
    let mut buy = buy_on("acc-const", "inst-const", 100.0, 0, 0, "2026-01-01");
    buy.amount_cents = 10_000;
    buy.price_cents = None;
    create_transaction_internal(&conn, buy).unwrap();
    create_transaction_internal(
        &conn,
        dividend_on("acc-const", "inst-const", 1_000, "2026-07-01"),
    )
    .unwrap();
    seed_price_history(&conn, "ph-flat", "inst-const", "2026-07-01", 20_000, "CNY");

    let range = MwrRange {
        start_date: Some("2026-07-01".into()),
        end_date: Some(TODAY.into()),
    };
    let summary = mwr_on(&conn, &range);
    assert_close(
        account_rate(&summary, "acc-const"),
        0.232_448_938_442_890_44,
    );
    assert_close(
        instrument_rate(&summary, "acc-const", "inst-const"),
        0.232_448_938_442_890_44,
    );
}
