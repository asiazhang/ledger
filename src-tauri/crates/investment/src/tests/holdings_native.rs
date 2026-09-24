//! 持仓逐行本位币列与累计收益折本位币单值（issue #1797）：持仓页签合计卡与
//! 首页投资概览卡的「统一单币种显示」读投影。
//!
//! 领域绑定判据：
//! - `list_holdings` 逐行补 `native_market_value_cents` / `native_unrealized_pnl_cents`：
//!   折算源 = 账户币种、当期汇率单点（`try_convert_to_native_current` 软形态）；
//!   单行缺当期汇率降为行级 `None`、命令不失败（表格的账户币展示不依赖折算）；
//!   缺价行保持 `v_holdings` 空值语义（两列 native 同为 `None`）；
//! - `query_cumulative_pnl_native_total`：三腿（未实现 + 已实现 + 分红）逐行按当期
//!   汇率折全局默认币种后求和，**硬形态**——缺汇率码化上抛 `fx.rate-missing`，不给
//!   半截数字；隐藏账户照常计入（Holding 口径，与投资概览页签的隐藏排除互为差异轴）；
//!   缺价行不进未实现腿（空值跳过、不以零计入）。

use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{
    open, seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_instrument,
    seed_market_price,
};

/// CNY（本位币）账户持仓：折算 1:1，native 列与原列逐位相等（同币种不走汇率表）。
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
    // 现价 120 元 → 市值 120_000 分、未实现 20_000 分（CNY = 本位币，1:1）。
    seed_market_price(&conn, "inst-cny", 1_200_000, "CNY");

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].market_value_cents, Some(120_000));
    assert_eq!(holdings[0].native_market_value_cents, Some(120_000));
    assert_eq!(holdings[0].unrealized_pnl_cents, Some(20_000));
    assert_eq!(holdings[0].native_unrealized_pnl_cents, Some(20_000));
}

/// 外币账户持仓按当期汇率逐行折算：native 列 = 原列 × 当期汇率（读当期表，
/// 与写路径的交易日历史折算互不取数）。
#[test]
fn holdings_native_columns_convert_at_current_rate() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    // 写路径（买入行金额归一）走交易日历史；读路径折算走当期表 7.0。
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2026-01-10"]);
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_instrument(&conn, "inst-usd", "AAPL", "Apple", "USD", "nasdaq");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-usd", "inst-usd", 2.0, 500_000, 0),
    )
    .unwrap();
    // 现价 60 美元 → 市值 12_000 美分、未实现 2_000 美分；折 CNY = ×7。
    seed_market_price(&conn, "inst-usd", 600_000, "USD");

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].market_value_cents, Some(12_000));
    assert_eq!(holdings[0].native_market_value_cents, Some(84_000));
    assert_eq!(holdings[0].unrealized_pnl_cents, Some(2_000));
    assert_eq!(holdings[0].native_unrealized_pnl_cents, Some(14_000));
}

/// 软形态：单行缺当期汇率降为行级 `None`、命令不失败——表格的账户币列示不依赖
/// 折算，缺料由展示面按「警告 + 重试」显式呈现，不静默混币种。
#[test]
fn holdings_native_columns_missing_rate_soft_null() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    // 只种交易日历史（写路径必需），不种当期表 → 读路径折算缺料。
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
    // 原列照常（账户币口径不受折算缺料影响），native 两列行级 None，命令不失败。
    assert_eq!(holdings[0].market_value_cents, Some(12_000));
    assert_eq!(holdings[0].unrealized_pnl_cents, Some(2_000));
    assert_eq!(holdings[0].native_market_value_cents, None);
    assert_eq!(holdings[0].native_unrealized_pnl_cents, None);
}

/// 缺价行保持 `v_holdings` 空值语义：原列 None，native 列同为 None（不以零计入）。
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
    // 不种现价 → v_holdings 市值 / 未实现为 NULL。

    let holdings = list_holdings(&conn).unwrap();
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].market_value_cents, None);
    assert_eq!(holdings[0].native_market_value_cents, None);
    assert_eq!(holdings[0].unrealized_pnl_cents, None);
    assert_eq!(holdings[0].native_unrealized_pnl_cents, None);
}

/// 累计收益折本位币单值：三腿逐行按当期汇率折算后求和（外币腿 ×当期汇率，
/// 本位币腿 1:1），native_currency 为全局默认币种。
#[test]
fn cumulative_pnl_native_total_converts_legs_at_current_rate() {
    let conn = open();
    // USD 腿：未实现 5_000 + 已实现 9_800 = 14_800 美分。
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
    // CNY 腿：分红 2_000 分（1:1）。
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

/// 隐藏账户照常计入（Holding 口径）：与投资概览页签的隐藏排除互为差异轴——
/// 持仓页签合计的口径不随账户可见性收窄。
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

/// 硬形态：任一腿缺当期汇率整条命令码化上抛 `fx.rate-missing`——合计要么完整、
/// 要么不给数，不静默给半截数字（展示面整卡警告 + 重试）。
#[test]
fn cumulative_pnl_native_total_missing_rate_raises_coded_error() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    // 写路径历史在场、当期表缺行 → 读路径折算缺料。
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

/// 缺价行不进未实现腿（`WHERE unrealized_pnl_cents IS NOT NULL`）：空值跳过、
/// 不以零计入，其余腿照常求和。
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
    // 无现价 → 未实现腿空；分红腿独立入合计。
    create_transaction_internal(
        &conn,
        make_dividend_input("acc-cny", "inst-cny", 2_500, "CNY"),
    )
    .unwrap();

    let total = query_cumulative_pnl_native_total(&conn).unwrap();
    assert_eq!(total.total_cents, 2_500);
}
