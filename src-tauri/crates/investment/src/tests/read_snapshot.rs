//! 多语句读闭包的快照一致性探针（issue #1699）：写提交落在语句之间时，同屏
//! 口径必须仍互相自洽——总量=分量和（realized_pnl）、分子分母同时点
//! （financial_freedom）。探针机制见 `tauri_app_lib::test_support::snapshot_probe`。

use ledger_transaction::create_transaction_internal;

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};
use tauri_app_lib::test_support::{
    FIXED_NOW, ScratchDir, open_file, seed_account, seed_exchange_rate, seed_fx_history_weeks,
    seed_instrument,
};

fn empty_filter() -> PnlFilter {
    PnlFilter {
        account_id: None,
        instrument_id: None,
    }
}

/// 总量（`total`）与三张分量表（按年 / 按账户 / 按标的）必须同快照：
/// 探针在按账户表查询开始前于另一连接改写匹配行已实现盈亏——
/// - 读闭包无快照保护（红）：total 读旧值、分量表读新值，「总量=分量和」变红；
/// - 读闭包收进读事务（绿）：注入写被挡住，四查同见一套数，断言绿。
#[test]
fn realized_pnl_total_equals_group_sums_under_concurrent_write() {
    let dir = ScratchDir::new("investment-pnl-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-pnl", "美股账户", "investment", "USD", 0);
    seed_fx_history_weeks(
        &conn,
        "USD",
        "CNY",
        1.0,
        &["2026-01-10", "2026-01-20", "2026-02-01", "2026-02-10"],
    );
    seed_instrument(&conn, "inst-pnl", "AAPL", "Apple", "USD", "unknown");

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

    // 探针：按账户表（`SELECT account_id, account_name, …`，与 total 的
    // `SELECT sls.currency_code…`、按年的 `SELECT year, …` 区分）开始前，
    // 另一连接提交匹配行改写。
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "SELECT account_id, account_name",
        &["UPDATE security_lot_sales SET realized_pnl_cents = realized_pnl_cents + 5000"],
    );

    let summary = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中按账户表查询（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    let total: i64 = summary.total.iter().map(|g| g.realized_pnl_cents).sum();
    let by_year: i64 = summary.by_year.iter().map(|r| r.realized_pnl_cents).sum();
    let by_account: i64 = summary
        .by_account
        .iter()
        .map(|r| r.realized_pnl_cents)
        .sum();
    let by_instrument: i64 = summary
        .by_instrument
        .iter()
        .map(|r| r.realized_pnl_cents)
        .sum();
    assert!(
        total > 0,
        "种子买卖应产出非零已实现盈亏（否则口径断言空转）"
    );
    assert_eq!(total, by_account, "总量必须等于按账户分量和（同快照）");
    assert_eq!(total, by_year, "总量必须等于按年分量和（同快照）");
    assert_eq!(total, by_instrument, "总量必须等于按标的部分和（同快照）");
}

/// 分子（可投资资产）与分母（年度预算总额）必须同时点：探针在分母 budgets
/// 聚合开始前于另一连接提交一条年度预算——两次读数之间库内唯一变动就是这笔
/// 注入写，逐字段相等即「分子分母同进同退」：
/// - 读闭包无快照保护（红）：分子读旧、分母读新，基线对拍变红；
/// - 读闭包收进读事务（绿）：注入写被挡住，两次读数逐字段相等。
#[test]
fn financial_freedom_numerator_and_denominator_share_timepoint() {
    let dir = ScratchDir::new("investment-ff-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-ff", "投资账户", "investment", "CNY", 200_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
    // 分母基线：一条月度预算（支出分类取迁移预置的确定性种子 id）。
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('budget-snap-baseline','95d6dc66-12c4-5f2b-bf9b-1d439a9c8100','monthly',100000,'2026-01-01',?1,?1,1,'test',0)",
        rusqlite::params![tauri_app_lib::test_support::FIXED_NOW],
    )
    .unwrap();

    let before = query_financial_freedom(&conn).unwrap();

    // 探针：分母聚合（`FROM budgets`，全闭包唯一命中）开始前，另一连接提交预算写
    //（簿记戳引 FIXED_NOW 常量，不落字面量——测试守门规则 3）。
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
