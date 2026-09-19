//! 投资概览读数（InvestmentOverview，spec #1532 / issue #1536）。
//!
//! 领域绑定判据（口径见词汇表「投资概览」与 ADR-0130）：
//! - 可投资资产 = 投资账户现金腿 + 持仓市值腿，且与既有单点
//!   `query_investable_assets_cents`（财务自由度分子 / 跨账本汇总同源）恒等——
//!   拆分不产生第二份口径表达式；
//! - 两腿各自折全局默认币种（负向：删掉任一条腿的折本位币折算 → 本例变红），
//!   缺汇率码化上抛；
//! - 隐藏账户的现金与持仓一并不计入、缺价持仓跳过但显式计数（不虚增也不低估）；
//! - 无投资账户时照常出 0 并给出「还没有投资账户」的引导事实。

use rusqlite::Connection;

use crate::overview::query_investment_overview;
use crate::prices::{MarketPriceWrite, upsert_market_price};
use ledger_transaction::{TransactionInput, create_transaction_internal};
use tauri_app_lib::test_support::{open, seed_account, seed_exchange_rate, seed_instrument};

use super::common::make_buy_input;

/// 币种显式的买入输入构造器：`make_buy_input` 固定 USD，多币种与 CNY 场景各自构造。
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

/// 种入一行现价缓存（价格写入单点，非裸 SQL）：`v_holdings` 据此算市值。
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

/// 人民币账户记账所需的 1:1 折算行（买入行金额归一需要它）。
fn cny_identity(conn: &Connection) {
    seed_exchange_rate(conn, "CNY", "CNY", 1.0);
}

/// 两腿之和 = 既有可投资资产单点；生活现金不计入；合计即两腿相加。
#[test]
fn overview_equals_two_legs_sum_binding_existing_single_point() {
    let conn = open();
    seed_account(&conn, "acc-inv", "投资账户", "investment", "CNY", 200_000);
    seed_account(&conn, "acc-life", "生活现金", "cash", "CNY", 999_999);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_instrument(&conn, "inst-a", "IA", "标的A", "CNY", "sh");
    cny_identity(&conn);
    // 买入无出资账户 → 结算账户 = 投资账户自己（ADR-0096）：现金 200_000 − 100_000。
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

/// 现金腿折全局默认币种（负向判据：删掉现金腿的折本位币折算 → 本用例变红）。
#[test]
fn overview_folds_cash_leg_by_account_currency() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 100_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);

    let overview = query_investment_overview(&conn).unwrap();
    assert_eq!(overview.native_currency, "CNY", "折算目标 = 全局默认币种");
    assert_eq!(
        overview.investment_cash_cents, 700_000,
        "1000 美元现金折 7.0 → 7000 元"
    );
    assert_eq!(overview.holdings_market_value_cents, 0);
    assert_eq!(overview.investable_assets_cents, 700_000);
}

/// 持仓市值腿折全局默认币种（负向判据：删掉持仓腿的折本位币折算 → 本用例变红）。
///
/// 账户余额 = 初始余额 + Σ 本位币流水（account_flow 口径，`v_holdings` 与
/// 财务自由度 BDD 同款）：买入 20000 美分折本位币流出 140000，初始 244000
/// 配平后账户现金恰余 104000 美分；市值腿只随现价与账户币种折算。
#[test]
fn overview_folds_holdings_leg_by_account_currency() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股券商", "investment", "USD", 244_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_instrument(&conn, "inst-nvda", "NVDA", "英伟达", "USD", "nasdaq");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    create_transaction_internal(
        &conn,
        buy_in("acc-usd", "inst-nvda", 2.0, 1_000_000, "USD", 0),
    )
    .unwrap();
    // 现价 150.00 美元 → 市值 2 × 150 = 300 美元 = 30000 美分（账户币）→ 折 210000 分。
    seed_price(&conn, "inst-nvda", 1_500_000, "USD");

    let overview = query_investment_overview(&conn).unwrap();
    assert_eq!(
        overview.holdings_market_value_cents, 210_000,
        "2 × 150 美元 × 7.0 → 2100 元"
    );
    assert_eq!(
        overview.investment_cash_cents, 728_000,
        "账户现金 104000 美分 × 7.0"
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

/// 缺折算汇率按既有码化错误上抛，不给半截数字。
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

/// 隐藏账户的现金与持仓一并不计入（含未计入计数），缺价持仓跳过但计数显式。
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
    // inst-bare 无现价：其 `v_holdings` 市值列 NULL → 跳过合计但计入未计入数。
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

/// 没有投资账户：照常出 0，并给出「还没有投资账户」的引导事实（不隐藏功能）。
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
