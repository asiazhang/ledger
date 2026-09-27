//! 已实现盈亏汇总（realized PnL）测试：空态、单笔 / 多账户聚合、按账户 / 按
//! 标的过滤、折本位币单值行（#1845，ADR-0107 决策 6 翻案）——逐腿按事件周
//! 汇率折算（卖出匹配按卖出周、现金分红腿按到账周）、就近周兜底接线、
//! 币对零历史码化错误、缺料行不以零计入、同币种恒等；两腿并入现金分红后的
//! 已实现收益口径沿用（ADR-0129）。

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

/// 主值与两腿拆解三件套断言（单值行形状，#1845）：Some 值逐位相等。
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
    // 逐腿按事件周汇率折算（#1845 核心口径）：卖出匹配按卖出周、现金分红腿
    // 按到账周——同一行内三个事件周各折各的汇率，不共用单一汇率。
    let conn = open();
    seed_account(&conn, "acc-week", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-week", "AAPL", "Apple", "USD", "unknown");
    // 三个事件周各一个汇率：卖出周 01-19 → 7.0、分红到账周 01-26 → 6.0、
    // 第二笔卖出周 02-09 → 7.5（买入周不取数——已实现腿只读卖出周）。
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2026-01-10", "2026-01-20"]);
    seed_fx_history_weeks(&conn, "USD", "CNY", 6.0, &["2026-02-01"]);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.5, &["2026-02-10"]);

    create_transaction_internal(
        &conn,
        make_buy_input("acc-week", "inst-week", 10.0, 1_000_000, 0),
    )
    .unwrap();
    // 卖 5@120 元 fee 2 元 → +9800 分，落在 01-20（周 01-19，汇率 7.0）
    create_transaction_internal(
        &conn,
        make_sell_input("acc-week", "inst-week", 5.0, 1_200_000, 200),
    )
    .unwrap();
    // 分红 1000 元（100000 分），落在 02-01（周 01-26，汇率 6.0）
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-week", "inst-week", 100_000, "USD", "2026-02-01"),
    )
    .unwrap();
    // 卖 5@130 元 → +15000 分，落在 02-10（周 02-09，汇率 7.5）
    create_transaction_internal(&conn, {
        let mut input = make_sell_input("acc-week", "inst-week", 5.0, 1_300_000, 0);
        input.date = "2026-02-10".into();
        input
    })
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.by_year.len(), 1);
    // 已实现腿 = 9800×7.0 + 15000×7.5 = 68600 + 112500 = 181100；
    // 分红腿 = 100000×6.0 = 600000（按到账周，不按卖出周）。
    assert_row(&result.by_year[0], "2026", 181_100, 600_000, 781_100, "CNY");
}

#[test]
fn realized_pnl_summary_nearest_week_fallback_wired_into_projection() {
    // 就近周兜底接线（#1845 验收：删除读投影中的就近周接线即红）：卖出周整周
    // 无点、前一周（窗口内最近）有点 → 按就近周折算；若投影只做精确周命中
    // （组合走势同款），该行会变「无法计算」，断言立即变红。
    let conn = open();
    seed_account(&conn, "acc-near", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-near", "AAPL", "Apple", "USD", "unknown");
    // 写入按交易日取数（写路径纪律），先种卖出周让交易落库，再删该周点——
    // 读侧看到的卖出周（01-19）整周无点，最近可用点是前一周 01-12（7.0）。
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
    // 9800 分 × 就近周（01-12）汇率 7.0 = 68600，而不是「无法计算」。
    assert_row(&result.by_year[0], "2026", 68_600, 0, 68_600, "CNY");
}

#[test]
fn realized_pnl_summary_missing_leg_beyond_window_marks_row_not_computable() {
    // 缺料语义（兜底三级终点）：兜底窗口（±8 周）内无任何点 → 该腿缺料，
    // 所在行显式「无法计算」——不以零计入、不给半截数字。
    let conn = open();
    seed_account(&conn, "acc-gap", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-gap", "AAPL", "Apple", "USD", "unknown");
    // 写入需卖出周汇率，先种后删：删后唯一历史点在卖出周（2026-01-19）前
    // 30 余周——窗口外，就近兜底救不了；01-05 距卖出周仅 2 周、必须一并删去。
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
    // 币对零历史（正反向均无行）沿用 fx.rate-missing 码化错误指路同步——与
    // 「窗口内仍无点」的缺料行（None）分属两条路径。
    let conn = open();
    seed_account(&conn, "acc-zero", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-zero", "AAPL", "Apple", "USD", "unknown");
    // 写入需要卖出周汇率（写路径纪律），先种后删：模拟「历史被清空的币对」。
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
    // 同币种恒等：CNY 账户在 CNY 本位币下不需要任何汇率历史——库内零汇率行
    // 也照常出数，数值与折算前逐位相同（单币种用户零变化）。
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
    // 币种维退役（#1845）：同一年度两币种折算后合成一行（旧按币种分组口径下
    // 是两行）；账户维保留，各自成行。
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

    // USD 账户：+9800 分 → ×7.0 = 68600
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
    // CNY 账户：−800 分（同币种恒等 ×1）
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

    // 按年一行：68600 + (−800) = 67800，不再按币种拆两行
    assert_eq!(result.by_year.len(), 1);
    assert_row(&result.by_year[0], "2026", 67_800, 0, 67_800, "CNY");
    // 按账户两行（账户维保留），账户名序：A 股账户在前
    assert_eq!(result.by_account.len(), 2);
    assert_eq!(result.by_account[0].account_id, "acc-cny");
    assert_eq!(result.by_account[0].realized_gain_cents, Some(-800));
    assert_eq!(result.by_account[1].account_id, "acc-usd");
    assert_eq!(result.by_account[1].realized_gain_cents, Some(68_600));
}

#[test]
fn realized_pnl_summary_dividend_only_year_appears() {
    // 只有分红、没有卖出的年份同样成行（ADR-0129 决策 1）：已实现腿为 0（无
    // 匹配行是「零」，不是「缺料」）、合计 = 分红腿。
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
    // 按账户表同理：只有分红的账户整行出现
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-dv");
    assert_eq!(result.by_account[0].realized_gain_cents, Some(836_536));
}

#[test]
fn realized_pnl_summary_merges_both_legs_in_same_year() {
    // 同年既有卖出又有分红：一行两腿、主值 = 两腿之和（ADR-0129 决策 1），
    // 已实现腿口径逐位不变（FIFO 匹配、不含分红——ADR-0109 决策 2）。
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
    // 排序沿用：按年后端升序返回（前端倒转展示），按账户按账户名序。
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
    // 分红腿与已实现腿同源同过滤（ADR-0129 决策 3）：账户筛选与标的筛选对两腿
    // 各用一次，两张表的行集同样随筛选收窄。
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

    // 账户与标的交叉未命中（既无卖出也无分红）→ 两表皆空，不出现零值行
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
    // 软删口径与累计收益三腿同源（ADR-0129 决策 3）：软删账户与软删分红流水
    // 排除；隐藏账户不是软删除（issue #217 定案 Q2），照常计入。
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

    // 隐藏账户计入：在用户 100.00 + 隐藏户 300.00 = 400.00 元；已删户的 200.00 不出现
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

    // 软删分红流水同样排除（ADR-0109 纠错路径：软删 + 重建）
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
    // 红利再投（ADR-0109 修订记录）：分红腿 + 0 费买入腿。分红计入本口径
    // （ADR-0129 决策 5），买入腿只抬持仓成本、不进本表（本页不展示未实现腿）。
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
    // 多币种分红同样折算合并（币种维退役，#1845）：USD 账户与 CNY 账户各收
    // 一笔分红，按年一行 = 两腿折本位币之和；账户维仍两行。
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
    // 软删账户口径（issue #217 定案）：删除账户 = 从全部投资视角（含已实现
    // 盈亏）消失，与 Holding / 时点持仓 / 走势对齐；恢复（标志翻回）自动回归。
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
    // 软删交易口径：sell 删除回补持仓并清空匹配行（ADR-0097），读口径排除
    // 仍保留以覆盖旧版本遗留的幽灵匹配（issue #940）。
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
