//! 投资概览读数（InvestmentOverview，spec #1532 / issue #1536）。
//!
//! 领域绑定判据（口径见词汇表「投资概览」与 ADR-0131）：
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
use crate::{query_cumulative_pnl_summary, query_holdings_summary_by_currency};
use ledger_transaction::amount::convert_to_native_current;
use ledger_transaction::{TransactionInput, create_transaction_internal};
use tauri_app_lib::test_support::{
    open, seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_instrument,
};

use super::common::{make_buy_input, make_dividend_input, make_sell_input};

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
    // 写路径按交易日取数（#1547）：另种交易周（工厂日期）的汇率历史点，
    // 当期行服务概览读侧折算。
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

/// 持仓市值腿折全局默认币种（负向判据：删掉持仓腿的折本位币折算 → 本用例变红）。
///
/// 账户余额 = 初始余额 + Σ **账户币种**流水（account_flow 口径，`v_holdings` 与
/// 财务自由度 BDD 同款）：买入 20000 美分（USD）自初始 244000 美分扣出，
/// 账户现金余 224000 美分（USD）；概览再按账户币种折本位币（× 7.0）。
#[test]
fn overview_folds_holdings_leg_by_account_currency() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股券商", "investment", "USD", 244_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_instrument(&conn, "inst-nvda", "NVDA", "英伟达", "USD", "nasdaq");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    // 写路径按交易日取数（#1547）：另种交易周（工厂日期）的汇率历史点，
    // 当期行服务概览读侧折算。
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
    // 现价 150.00 美元 → 市值 2 × 150 = 300 美元 = 30000 美分（账户币）→ 折 210000 分。
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

/// 投资合计三项折全局默认币种（#1537；负向判据：删掉三项的折本位币聚合 → 本用例
/// 变红）：总市值 = 持仓市值腿（同一聚合）；持仓收益 = 未实现盈亏；累计收益 =
/// 持仓收益 + 已实现盈亏 + 累计分红——三项均与既有分组口径同源（无隐藏账户时
/// 与「分组求和再折算」恒等，绑定断言钉住同源不漂移）。
#[test]
fn overview_totals_fold_to_native_currency() {
    let conn = open();
    // 买 2 股 @ $100 → 卖 1 股 @ $120（已实现 +$20）→ 余 1 股成本 $100；
    // 现价 $150 → 市值 $150、未实现 +$50；现金分红 $30。
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-x", "XX", "标的X", "USD", "nasdaq");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    // 写路径按交易日取数（#1547）：另种交易周（工厂日期）的汇率历史点，
    // 当期行服务概览读侧折算。
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
    // 总市值与持仓市值腿在全页口径下是同一聚合（同取数面同折算），恒等绑定。
    assert_eq!(
        overview.total_market_value_cents,
        overview.holdings_market_value_cents
    );

    // 与既有分组口径同源（同源不漂移）：无隐藏账户时「分组求和再逐组折算」与
    // 本页逐行折算在单币种组下恒等。
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

/// 投资合计三项同样排除隐藏账户（页面级边界「同 InvestableAssets 口径」，词汇表
/// 「投资概览」）：隐藏账户的持仓、已实现与分红一并不进三项；既有分组口径仍含
/// 隐藏账户（绑定断言钉住差异轴恰为隐藏账户）。缺价持仓照常跳过并计数。
#[test]
fn overview_totals_exclude_hidden_accounts() {
    let conn = open();
    // 可见账户：买 1 股 @ ¥100，现价 ¥110 → 市值 ¥110、未实现 +¥10；另持一只缺价标的。
    seed_account(&conn, "acc-vis", "可见账户", "investment", "CNY", 50_000);
    seed_instrument(&conn, "inst-vis", "VIS", "可见标的", "CNY", "sh");
    seed_instrument(&conn, "inst-bare", "BARE", "缺价标的", "CNY", "sh");
    // 隐藏账户：买 2 股 @ ¥100 → 卖 1 股 @ ¥90（已实现 −¥10）→ 现价 ¥120 →
    // 市值 ¥120、未实现 +¥20；分红 ¥50。
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

    // 既有分组口径含隐藏账户：三项与之的差恰为隐藏账户的贡献（差异轴唯一）。
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

/// 已实现/分红腿与未实现腿同走单一折算点：任一腿缺折算汇率即整命令码化上抛
/// （前端整卡警告 + 重试，不给半截数字）。
#[test]
fn overview_totals_missing_rate_raises_coded_error() {
    let conn = open();
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-y", "YY", "标的Y", "USD", "nasdaq");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    // 写路径按交易日取数（#1547）：另种交易周（工厂日期）的汇率历史点，
    // 当期行服务概览读侧折算。
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
    // 写入时汇率在位（买入归一需要它），读数时撤走：本页按当期汇率折算，
    // 读时缺汇率即失败——不静默回退、不给半截数字。
    conn.execute("DELETE FROM exchange_rates", []).unwrap();

    let err = query_investment_overview(&conn).unwrap_err();
    assert!(
        err.is_code("fx.rate-missing"),
        "缺汇率应报 fx.rate-missing，实际 {err:?}"
    );
}
