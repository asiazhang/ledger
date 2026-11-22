use ledger_transaction::create_transaction_internal;

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};
use tauri_app_lib::test_support::{
    FIXED_NOW, ScratchDir, open_file, seed_account, seed_exchange_rate, seed_fx_history_weeks,
    seed_instrument, seed_market_price, seed_price_history,
};

fn empty_filter() -> PnlFilter {
    PnlFilter {
        account_id: None,
        instrument_id: None,
    }
}

#[test]
fn realized_pnl_two_tables_share_one_snapshot_under_concurrent_write() {
    let dir = ScratchDir::new("investment-pnl-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-pnl", "A股账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-pnl", "600000", "浦发银行", "CNY", "sh");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-pnl", "inst-pnl", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-pnl", "inst-pnl", 5.0, 1_200_000, 200),
    )
    .unwrap();

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "SELECT a.id, a.name, sls.currency_code",
        &["UPDATE security_lot_sales SET realized_pnl_cents = realized_pnl_cents + 5000"],
    );

    let summary = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中按账户腿取数（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    let realized_of =
        |rows: &[YearPnl]| -> i64 { rows.iter().map(|r| r.realized_pnl_cents.unwrap_or(0)).sum() };
    let by_year = realized_of(&summary.by_year);
    let by_account: i64 = summary
        .by_account
        .iter()
        .map(|r| r.realized_pnl_cents.unwrap_or(0))
        .sum();
    assert!(
        by_year > 0,
        "种子买卖应产出非零已实现盈亏（否则口径断言空转）"
    );
    assert_eq!(
        by_year, by_account,
        "两表折算自同一份已实现腿，必须同快照（注入写落在两表取数之间即漂移）"
    );
}

#[test]
fn financial_freedom_numerator_and_denominator_share_timepoint() {
    let dir = ScratchDir::new("investment-ff-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-ff", "投资账户", "investment", "CNY", 200_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('budget-snap-baseline','95d6dc66-12c4-5f2b-bf9b-1d439a9c8100','monthly',100000,'2026-01-01',?1,?1,1,'test',0)",
        rusqlite::params![tauri_app_lib::test_support::FIXED_NOW],
    )
    .unwrap();

    let before = query_financial_freedom(&conn).unwrap();

    let inject_budget = format!(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('budget-snap-extra','95d6dc66-12c4-5f2b-bf9b-1d439a9c8100','yearly',666000,'2026-01-01','{FIXED_NOW}','{FIXED_NOW}',1,'probe',0)"
    );
    snapshot_probe::arm(&conn, dir.path(), "FROM budgets", &[&inject_budget]);

    let after = query_financial_freedom(&conn).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中分母聚合（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert!(
        before.denominator_cents > 0,
        "种子预算应产出非零分母（否则口径断言空转）"
    );
    assert_eq!(
        before.numerator_cents, after.numerator_cents,
        "分子必须与基线同时点（注入写落在语句之间时分子读旧、分母读新）"
    );
    assert_eq!(
        before.denominator_cents, after.denominator_cents,
        "分母必须与基线同时点——注入写要么整体进快照、要么整体不进"
    );
    assert_eq!(
        before.ratio, after.ratio,
        "自由度百分比随分子分母漂移即两端不同时点"
    );
    assert_eq!(
        before.coverage_years, after.coverage_years,
        "覆盖年数随分子分母漂移即两端不同时点"
    );
    assert_eq!(
        before.native_currency, after.native_currency,
        "折算基准币种不应漂移"
    );
}

#[test]
fn investment_overview_legs_share_one_snapshot() {
    let dir = ScratchDir::new("investment-overview-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-ov", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
    seed_instrument(&conn, "inst-ov", "AAPL", "Apple", "USD", "unknown");
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    create_transaction_internal(
        &conn,
        make_buy_input("acc-ov", "inst-ov", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-ov", "inst-ov", 5.0, 1_200_000, 200),
    )
    .unwrap();

    let before = query_investment_overview(&conn).unwrap();

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "SELECT sls.realized_pnl_cents",
        &["UPDATE security_lot_sales SET realized_pnl_cents = realized_pnl_cents + 5000"],
    );
    let after = query_investment_overview(&conn).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中已实现腿（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert!(
        before.cumulative_pnl_cents > 0,
        "种子买卖应产出非零累计收益（否则口径断言空转）"
    );
    assert_eq!(
        before.cumulative_pnl_cents, after.cumulative_pnl_cents,
        "累计收益（持仓收益+已实现+分红三腿）必须同快照"
    );
    assert_eq!(
        before.investable_assets_cents, after.investable_assets_cents,
        "可投资资产（现金+持仓两腿）必须同快照"
    );
    assert_eq!(
        before.native_currency, after.native_currency,
        "折算基准币种不应漂移"
    );
}

#[test]
fn mwr_flows_and_end_value_share_one_snapshot() {
    let dir = ScratchDir::new("investment-mwr-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-mwr", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 1.0, &["2026-01-10"]);
    seed_instrument(&conn, "inst-mwr", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-mwr", "inst-mwr", 10.0, 1_000_000, 0),
    )
    .unwrap();
    seed_market_price(&conn, "inst-mwr", 1_100_000, "USD");

    let before = query_money_weighted_return_summary(&conn, &MwrRange::default()).unwrap();

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM v_holdings v JOIN accounts a",
        &["UPDATE market_prices SET price_cents = price_cents * 2"],
    );
    let after = query_money_weighted_return_summary(&conn, &MwrRange::default()).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中期末市值读取（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(before.total.len(), 1, "种子应产出恰好一个币种组");
    assert_eq!(
        before.total[0].basis,
        MwrBasis::Annualized,
        "有真实流水的对应给年化口径（否则口径断言空转）"
    );
    assert!(
        before.total[0].rate.is_some_and(|r| r > 0.0),
        "现价高出买入价应产出正收益率（否则口径断言空转）"
    );
    assert_eq!(
        before
            .by_instrument
            .iter()
            .map(|r| (
                r.account_id.as_str(),
                r.instrument_id.as_str(),
                r.currency_code.as_str(),
                r.basis,
                r.rate
            ))
            .collect::<Vec<_>>(),
        after
            .by_instrument
            .iter()
            .map(|r| (
                r.account_id.as_str(),
                r.instrument_id.as_str(),
                r.currency_code.as_str(),
                r.basis,
                r.rate
            ))
            .collect::<Vec<_>>(),
        "单标的收益率必须与基线同时点（注入写落在现金流与期末市值之间即漂移）"
    );
    assert_eq!(
        before
            .by_account
            .iter()
            .map(|r| (
                r.account_id.as_str(),
                r.account_name.as_str(),
                r.currency_code.as_str(),
                r.basis,
                r.rate
            ))
            .collect::<Vec<_>>(),
        after
            .by_account
            .iter()
            .map(|r| (
                r.account_id.as_str(),
                r.account_name.as_str(),
                r.currency_code.as_str(),
                r.basis,
                r.rate
            ))
            .collect::<Vec<_>>(),
        "账户级收益率必须与基线同时点"
    );
    assert_eq!(
        before
            .total
            .iter()
            .map(|r| (r.currency_code.as_str(), r.basis, r.rate))
            .collect::<Vec<_>>(),
        after
            .total
            .iter()
            .map(|r| (r.currency_code.as_str(), r.basis, r.rate))
            .collect::<Vec<_>>(),
        "全账级收益率必须与基线同时点"
    );
}

#[test]
fn cumulative_pnl_native_total_legs_and_fx_share_one_snapshot() {
    let dir = ScratchDir::new("investment-cpnt-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-cpnt", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_instrument(&conn, "inst-cpnt", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-cpnt", "inst-cpnt", 10.0, 1_000_000, 0),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_sell_input("acc-cpnt", "inst-cpnt", 5.0, 1_200_000, 200),
    )
    .unwrap();
    seed_market_price(&conn, "inst-cpnt", 1_100_000, "USD");

    let before = query_cumulative_pnl_native_total(&conn).unwrap();
    assert_eq!(
        before.total_cents, 103_600,
        "种子三腿 ×当期汇率 7 应得 103_600 分（否则口径断言空转）"
    );

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "SELECT rate FROM exchange_rates",
        &["UPDATE exchange_rates SET rate = rate * 2"],
    );
    let after = query_cumulative_pnl_native_total(&conn).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中三腿取数（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        before.total_cents, after.total_cents,
        "腿金额与折算汇率必须同快照（金额读旧、汇率读新即漂移）"
    );
    assert_eq!(
        before.native_currency, after.native_currency,
        "折算基准币种不应漂移"
    );
}

#[test]
fn portfolio_trend_prices_and_fx_share_one_snapshot() {
    let dir = ScratchDir::new("investment-trend-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-trd-snap", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        7.0,
        &["2026-01-10", "2026-02-02", "2026-02-09"],
    );
    seed_instrument(&conn, "inst-trd-snap", "AAPL", "Apple", "USD", "unknown");
    seed_price_history(
        &conn,
        "ph-s1",
        "inst-trd-snap",
        "2026-02-02",
        100_000,
        "USD",
    );
    seed_price_history(
        &conn,
        "ph-s2",
        "inst-trd-snap",
        "2026-02-09",
        200_000,
        "USD",
    );
    create_transaction_internal(
        &conn,
        make_buy_input("acc-trd-snap", "inst-trd-snap", 10.0, 100_000, 0),
    )
    .unwrap();

    let before = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(
        before
            .points
            .iter()
            .map(|p| p.market_value_cents)
            .collect::<Vec<_>>(),
        [70_000, 140_000],
        "种子曲线应为 10 股 × 周价 × 汇率 7（否则口径断言空转）"
    );

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM fx_rate_history",
        &["UPDATE fx_rate_history SET rate = rate * 2"],
    );
    let after = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中汇率历史读取（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        before
            .points
            .iter()
            .map(|p| (p.date.as_str(), p.market_value_cents))
            .collect::<Vec<_>>(),
        after
            .points
            .iter()
            .map(|p| (p.date.as_str(), p.market_value_cents))
            .collect::<Vec<_>>(),
        "曲线各周必须与基线同时点（价格读旧、汇率读新即漂移）"
    );
    assert_eq!(
        before.currency_code, after.currency_code,
        "折算基准币种不应漂移"
    );
}
