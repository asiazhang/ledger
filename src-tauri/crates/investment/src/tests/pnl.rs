//! 已实现盈亏汇总（realized PnL）测试：空态、单笔 / 多账户聚合、按账户 / 按
//! 标的过滤、按币种分组不混算（ADR-0107；issue #257 纯移动归组），
//! 两表并入现金分红后的已实现收益口径（ADR-0129 / issue #1533），
//! 以及按年表的完整年度收益（未实现变动腿，ADR-0132 / issue #1535）。

use ledger_transaction::SecurityOrigin;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{
    open, seed_account, seed_fx_history_weeks, seed_instrument, seed_price_history,
};

fn empty_filter() -> PnlFilter {
    PnlFilter {
        account_id: None,
        instrument_id: None,
    }
}

/// 取指定币种的分组小计（ADR-0107 决策 6：汇总按匹配行币种分组，不混算）。
fn total_for(summary: &RealizedPnlSummary, currency: &str) -> i64 {
    summary
        .total
        .iter()
        .find(|g| g.currency_code == currency)
        .map(|g| g.realized_pnl_cents)
        .unwrap_or(0)
}

#[test]
fn realized_pnl_summary_empty_when_no_sales() {
    let conn = open();
    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert!(result.total.is_empty());
    assert!(result.by_year.is_empty());
    assert!(result.by_account.is_empty());
    assert!(result.by_instrument.is_empty());
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

    assert_eq!(total_for(&result, "USD"), 9800);
    assert_eq!(result.by_year.len(), 1);
    assert_eq!(result.by_year[0].currency_code, "USD");
    assert_eq!(result.by_year[0].realized_pnl_cents, 9800);
    // 无分红场景读数逐位不变（ADR-0129 验收）：分红腿恒 0、合计 = 已实现盈亏
    assert_eq!(result.by_year[0].dividend_cents, 0);
    assert_eq!(result.by_year[0].realized_gain_cents, 9800);
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-pnl");
    assert_eq!(result.by_account[0].currency_code, "USD");
    assert_eq!(result.by_account[0].realized_pnl_cents, 9800);
    assert_eq!(result.by_account[0].dividend_cents, 0);
    assert_eq!(result.by_account[0].realized_gain_cents, 9800);
    assert_eq!(result.by_instrument.len(), 1);
    assert_eq!(result.by_instrument[0].instrument_id, "inst-pnl");
    assert_eq!(result.by_instrument[0].symbol, "AAPL");
    assert_eq!(result.by_instrument[0].realized_pnl_cents, 9800);
}

#[test]
fn realized_pnl_summary_dividend_only_year_appears() {
    // 只有分红、没有卖出的年份同样成行（ADR-0129 决策 1）：真实账本「老婆的且慢」
    // 2019 年即此形态（已实现 0.00、现金分红 8,365.36），原口径下这一行整行不存在。
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
    assert_eq!(result.by_year[0].year, "2019");
    assert_eq!(result.by_year[0].currency_code, "CNY");
    assert_eq!(result.by_year[0].realized_pnl_cents, 0);
    assert_eq!(result.by_year[0].dividend_cents, 836_536);
    assert_eq!(result.by_year[0].realized_gain_cents, 836_536);
    // 完整年度收益（ADR-0132 / issue #1535 验收）：当年无持仓 → 未实现变动 0、
    // 年度收益 = 分红腿（与且慢 App 显示一致）
    assert_eq!(result.by_year[0].unrealized_change_cents, Some(0));
    assert_eq!(result.by_year[0].annual_return_cents, Some(836_536));
    // 按账户表同理：只有分红的账户整行出现（原口径下同样缺席）
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-dv");
    assert_eq!(result.by_account[0].realized_pnl_cents, 0);
    assert_eq!(result.by_account[0].dividend_cents, 836_536);
    assert_eq!(result.by_account[0].realized_gain_cents, 836_536);
    // 按币种总数维持已实现盈亏专义（ADR-0129 决策 4）：无卖出即无行，不被分红撑出空组
    assert!(result.total.is_empty());
    assert!(result.by_instrument.is_empty());
}

#[test]
fn realized_pnl_summary_merges_both_legs_in_same_year() {
    // 同年既有卖出又有分红：一行两腿、合计 = 两腿之和（ADR-0129 决策 1），
    // 而已实现腿口径逐位不变（FIFO 匹配、不含分红——ADR-0109 决策 2）。
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
    assert_eq!(result.by_year[0].year, "2021");
    // 卖出 5 份，每份 100.00 → 120.00 元（价格刻度万分之一元）：已实现 100.00 元整
    assert_eq!(result.by_year[0].realized_pnl_cents, 10_000);
    assert_eq!(result.by_year[0].dividend_cents, 126_318);
    assert_eq!(result.by_year[0].realized_gain_cents, 136_318);
    // 同一年同一币种只成一行（两腿在二次聚合里合并，不是两行）
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].realized_gain_cents, 136_318);
}

#[test]
fn realized_pnl_summary_dividend_leg_follows_filters() {
    // 分红腿与已实现腿同源同过滤（ADR-0129 决策 3）：账户筛选与标的筛选对两腿各用一次，
    // 两张表的行集同样随筛选收窄。
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
    assert_eq!(by_account.by_account[0].dividend_cents, 100_000);
    assert_eq!(by_account.by_year.len(), 1);
    assert_eq!(by_account.by_year[0].dividend_cents, 100_000);

    let by_instrument = query_realized_pnl_summary(
        &conn,
        &PnlFilter {
            account_id: None,
            instrument_id: Some("inst-b".into()),
        },
    )
    .unwrap();
    assert_eq!(by_instrument.by_year.len(), 1);
    assert_eq!(by_instrument.by_year[0].dividend_cents, 200_000);
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
    // 软删口径与累计收益三腿同源（ADR-0129 决策 3）：软删账户与软删分红流水排除；
    // 隐藏账户不是软删除（issue #217 定案 Q2），照常计入。
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
            .map(|a| a.dividend_cents)
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
    assert_eq!(after.by_account[0].dividend_cents, 100_000);
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
    assert_eq!(result.by_year[0].realized_pnl_cents, 0);
    assert_eq!(result.by_year[0].dividend_cents, 123_180);
    assert_eq!(result.by_year[0].realized_gain_cents, 123_180);
}

#[test]
fn realized_pnl_summary_groups_dividend_by_currency() {
    // 分红腿按交易行币种分组、不与另一币种相加（ADR-0129 决策 3 沿用 ADR-0107 决策 6）：
    // 同一年、两币种 → 两行，各自独立成立。
    let conn = open();
    seed_account(&conn, "acc-cny", "人民币户", "investment", "CNY", 0);
    seed_account(&conn, "acc-usd", "美元户", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 1.0, &["2021-03-01"]);
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
    assert_eq!(result.by_year.len(), 2);
    let cny = result
        .by_year
        .iter()
        .find(|r| r.currency_code == "CNY")
        .unwrap();
    let usd = result
        .by_year
        .iter()
        .find(|r| r.currency_code == "USD")
        .unwrap();
    assert_eq!(cny.dividend_cents, 100_000);
    assert_eq!(usd.dividend_cents, 250_000);
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

    assert_eq!(total_for(&result, "USD"), 3000);
    assert_eq!(result.by_account.len(), 2);
    assert_eq!(result.by_account[0].account_id, "acc-a");
    assert_eq!(result.by_account[0].realized_pnl_cents, 2000);
    assert_eq!(result.by_account[1].account_id, "acc-b");
    assert_eq!(result.by_account[1].realized_pnl_cents, 1000);
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

    assert_eq!(total_for(&result, "USD"), 2000);
    assert_eq!(result.by_account.len(), 1);
}

#[test]
fn realized_pnl_summary_excludes_soft_deleted_account() {
    // 软删账户口径（issue #217 定案）：删除账户 = 从全部投资视角（含已实现盈亏）
    // 消失，与 Holding / 时点持仓 / 走势对齐；恢复（标志翻回）自动回归，口径可逆。
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
    assert_eq!(total_for(&result, "USD"), 2000);
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-live");
    assert_eq!(result.by_account[0].account_name, "在用户");
}

#[test]
fn realized_pnl_summary_excludes_soft_deleted_sell() {
    // 软删交易口径：sell 删除不清理匹配行（ADR-0013 锁定既有行为），排除发生在
    // 读口径——软删 sell 的已实现盈亏不再计入汇总，与软删账户同原则。
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
    assert_eq!(total_for(&baseline, "USD"), 2000);

    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        rusqlite::params![sell_id],
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert!(result.total.is_empty());
    assert!(result.by_year.is_empty());
    assert!(result.by_account.is_empty());
    assert!(result.by_instrument.is_empty());
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

    assert_eq!(total_for(&result, "USD"), 2000);
    assert_eq!(result.by_instrument.len(), 1);
    assert_eq!(result.by_instrument[0].instrument_id, "inst-a");
}

#[test]
fn realized_pnl_summary_groups_by_currency_without_mixing() {
    // 按币种分组（ADR-0107 决策 6）：两种币种的匹配行独立成组，不跨币种相加——
    // 原「各币种裸数字直接 SUM」的混算口径在多币种账户下合计是错的。
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

    // USD 账户：买 10@100 元、卖 5@120 元 fee 2 元 → +98 元（9800 分）
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
    // CNY 账户：买 5@10 元、卖 2@6 元 → −8 元（−800 分，同 2026 年）
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

    // 两个币种各成一组：USD +9800、CNY −800，不出现混算后的 9000
    assert_eq!(result.total.len(), 2);
    assert_eq!(total_for(&result, "USD"), 9800);
    assert_eq!(total_for(&result, "CNY"), -800);
    // 同一年度、不同账户/标的，各按币种拆成两行
    assert_eq!(result.by_year.len(), 2);
    assert_eq!(result.by_account.len(), 2);
    assert_eq!(result.by_instrument.len(), 2);
}

// ---------------------------------------------------------------------------
// 完整年度收益（未实现变动腿）：ADR-0132 / issue #1535
// ---------------------------------------------------------------------------

#[test]
fn annual_return_identity_with_boundary_prices() {
    // 三段式明细钉住恒等式（issue #1535 验收）：已实现为负、分红为正、浮盈为正的
    // 年份（2024 形态）——年度收益 = 已实现 + 分红 + 未实现变动 = (期末市值 − 期初
    // 市值) + 卖出净收入 + 分红 − 买入支出。
    let conn = open();
    seed_account(&conn, "acc-ar", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ar", "501000", "基金A", "CNY", "unknown");
    // 年界周采样点（万分之一元刻度）：2023-12-31 = 100 元、2024-12-31 = 110 元
    seed_price_history(&conn, "ph-start", "inst-ar", "2023-12-31", 1_000_000, "CNY");
    seed_price_history(&conn, "ph-end", "inst-ar", "2024-12-31", 1_100_000, "CNY");

    // 2023 年建仓 10 份 @100 元（只买入、无卖出/分红的年份不成行）
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ar",
            "inst-ar",
            10.0,
            1_000_000,
            "2023-12-20",
        ),
    )
    .unwrap();
    // 2024 年：卖 4 份 @90 元（已实现 −40 元）、分红 1000 元、再买 5 份 @105 元
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-ar",
            "inst-ar",
            4.0,
            900_000,
            "2024-06-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-ar", "inst-ar", 100_000, "CNY", "2024-08-10"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-ar",
            "inst-ar",
            5.0,
            1_050_000,
            "2024-09-10",
        ),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    // 2023 只买入不成行（行集与 #1533 逐位一致，只加列不加行）
    assert_eq!(result.by_year.len(), 1);
    let row = &result.by_year[0];
    assert_eq!(row.year, "2024");
    assert_eq!(row.currency_code, "CNY");
    // 已实现：卖 360 元 − FIFO 成本 400 元 = −40 元
    assert_eq!(row.realized_pnl_cents, -4_000);
    assert_eq!(row.dividend_cents, 100_000);
    assert_eq!(row.realized_gain_cents, 96_000);
    // 未实现变动 = (期末 1210 元 − 期初 1000 元) + 卖出 360 − 买入 525 − (−40)
    //            = 期末浮盈 85 元（期初浮盈 0）
    assert_eq!(row.unrealized_change_cents, Some(8_500));
    assert_eq!(row.annual_return_cents, Some(104_500));
    // 现金流式恒等式（分）：(121_000 − 100_000) + 36_000 + 100_000 − 52_500
    assert_eq!(
        row.annual_return_cents,
        Some(121_000 - 100_000 + 36_000 + 100_000 - 52_500)
    );
}

#[test]
fn annual_return_not_computable_when_boundary_price_missing() {
    // 缺价年份标「不可算」（None），不静默按 0 计；已实现与分红两腿照常出数
    // （不受行情影响）。期初无持仓、期末持仓 10 份而年末无价 → 期末市值缺料。
    let conn = open();
    seed_account(&conn, "acc-np", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-np", "501000", "基金A", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-np",
            "inst-np",
            10.0,
            1_000_000,
            "2024-03-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-np", "inst-np", 50_000, "CNY", "2024-05-01"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_year.len(), 1);
    let row = &result.by_year[0];
    assert_eq!(row.realized_pnl_cents, 0);
    assert_eq!(row.dividend_cents, 50_000);
    assert_eq!(row.realized_gain_cents, 50_000);
    assert_eq!(row.unrealized_change_cents, None);
    assert_eq!(row.annual_return_cents, None);
}

#[test]
fn annual_return_computable_without_prices_when_no_boundary_holdings() {
    // 年内全平仓（期初/期末持仓均为 0）→ 无需任何价格即可算：未实现变动 0、
    // 年度收益 = 已实现 + 分红（市值不可知也不影响该年口径）。
    let conn = open();
    seed_account(&conn, "acc-rt", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-rt", "501000", "基金A", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-rt",
            "inst-rt",
            10.0,
            1_000_000,
            "2021-02-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-rt",
            "inst-rt",
            10.0,
            1_200_000,
            "2021-06-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-rt", "inst-rt", 5_000, "CNY", "2021-07-01"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_year.len(), 1);
    let row = &result.by_year[0];
    assert_eq!(row.realized_pnl_cents, 20_000);
    assert_eq!(row.dividend_cents, 5_000);
    assert_eq!(row.unrealized_change_cents, Some(0));
    assert_eq!(row.annual_return_cents, Some(25_000));
}

#[test]
fn annual_return_convert_is_cash_free_and_value_continuous() {
    // 基金转换无现金腿、市值连续（issue #1535 口径）：期初持 A、年中全换 B、
    // 期末持 B —— 转换不入现金流，年度收益 = 期末 B 市值 − 期初 A 市值 + 分红腿。
    // A 只需期初价、B 只需期末价（各边界只估值当时的持仓标的）。
    let conn = open();
    seed_account(&conn, "acc-cv", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-a", "501000", "基金A", "CNY", "unknown");
    seed_instrument(&conn, "inst-b", "502000", "基金B", "CNY", "unknown");
    seed_price_history(&conn, "ph-a", "inst-a", "2023-12-31", 1_000_000, "CNY");
    seed_price_history(&conn, "ph-b", "inst-b", "2024-12-31", 600_000, "CNY");

    // 2023 年买 A 10 份 @100 元；2024 年年中全换 B（结转成本 1000 元 → 20 份 @50 元）
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-cv",
            "inst-a",
            10.0,
            1_000_000,
            "2023-06-01",
        ),
    )
    .unwrap();
    let mut cv = make_convert_input(
        "acc-cv", "inst-a", "inst-b", 10.0, 20.0, 100_000, 100_000, 0,
    );
    cv.date = "2024-06-01".into();
    create_transaction_internal(&conn, cv).unwrap();
    // 转换年无卖出/分红不成行：补一笔分红让 2024 成行（行集纪律：只加列不加行）
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-cv", "inst-b", 10_000, "CNY", "2024-08-01"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_year.len(), 1);
    let row = &result.by_year[0];
    assert_eq!(row.year, "2024");
    // 未实现变动 = 期末 B 市值 1200 元 − 期初 A 市值 1000 元（转换零现金腿）
    assert_eq!(row.unrealized_change_cents, Some(20_000));
    assert_eq!(row.annual_return_cents, Some(30_000));
}

#[test]
fn annual_return_counts_opening_balance_buy_as_expenditure() {
    // 期初存量（补记 buy，origin='opening'）是 buy 行、计入年内买入支出
    //（issue #1535 口径按字面：「该年 buy 行金额合计」）：否则补记年会把整笔
    // 存量市值虚计为年度收益。未实现变动 = 期末市值 − 补记成本（自补记日起算）。
    let conn = open();
    seed_account(&conn, "acc-ob", "投资户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-ob", "501000", "基金A", "CNY", "unknown");
    seed_price_history(&conn, "ph-ob", "inst-ob", "2024-12-31", 1_050_000, "CNY");

    let mut opening = make_trade_input(
        TransactionKind::Buy,
        "acc-ob",
        "inst-ob",
        10.0,
        1_000_000,
        "2024-03-01",
    );
    opening.origin = Some(SecurityOrigin::Opening);
    create_transaction_internal(&conn, opening).unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-ob", "inst-ob", 10_000, "CNY", "2024-05-01"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_year.len(), 1);
    let row = &result.by_year[0];
    // 未实现变动 = 期末市值 1050 元 − 补记成本 1000 元 = 50 元（若补记买入不入
    // 支出，该腿会虚增为 1050 元）
    assert_eq!(row.unrealized_change_cents, Some(5_000));
    assert_eq!(row.annual_return_cents, Some(15_000));
}

#[test]
fn annual_return_legs_follow_filters() {
    // 未实现变动腿与已实现/分红腿同源同过滤（ADR-0132）：账户筛选收窄后只估值
    // 该账户名下持仓——无价账户整行不可算，筛到有价账户即可算。
    let conn = open();
    seed_account(&conn, "acc-fa", "有价户", "investment", "CNY", 0);
    seed_account(&conn, "acc-fb", "缺价户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-fa", "501000", "基金A", "CNY", "unknown");
    seed_instrument(&conn, "inst-fb", "502000", "基金B", "CNY", "unknown");
    seed_price_history(&conn, "ph-fa", "inst-fa", "2024-12-31", 1_000_000, "CNY");

    for (acc, inst) in [("acc-fa", "inst-fa"), ("acc-fb", "inst-fb")] {
        create_transaction_internal(
            &conn,
            make_trade_input(
                TransactionKind::Buy,
                acc,
                inst,
                10.0,
                1_000_000,
                "2024-03-01",
            ),
        )
        .unwrap();
        create_transaction_internal(
            &conn,
            make_dividend_input_on(acc, inst, 10_000, "CNY", "2024-05-01"),
        )
        .unwrap();
    }

    // 不过滤：缺价户的期末持仓拉低整行 → 不可算（同年同币种一行）
    let all = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(all.by_year.len(), 1);
    assert_eq!(all.by_year[0].unrealized_change_cents, None);

    // 筛到有价户：可算（期初持仓 0、期末市值 1000 元、买入 1000 元 → 未实现 0）
    let filtered = query_realized_pnl_summary(
        &conn,
        &PnlFilter {
            account_id: Some("acc-fa".into()),
            instrument_id: None,
        },
    )
    .unwrap();
    assert_eq!(filtered.by_year.len(), 1);
    assert_eq!(filtered.by_year[0].unrealized_change_cents, Some(0));
    assert_eq!(filtered.by_year[0].annual_return_cents, Some(10_000));

    // 标的筛选同理
    let by_inst = query_realized_pnl_summary(
        &conn,
        &PnlFilter {
            account_id: None,
            instrument_id: Some("inst-fa".into()),
        },
    )
    .unwrap();
    assert_eq!(by_inst.by_year.len(), 1);
    assert_eq!(by_inst.by_year[0].unrealized_change_cents, Some(0));
}

#[test]
fn annual_return_missing_fx_at_boundary_is_not_computable() {
    // 缺同期汇率同按缺料处置（ADR-0132 空值语义）：美元价标的持在人民币户，
    // 期末周点缺 USD→CNY 汇率 → 该年不可算。
    let conn = open();
    seed_account(&conn, "acc-fx", "人民币户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-fx", "AAPL", "Apple", "USD", "unknown");
    // 交易周（2024-03-04 当周）有汇率（写路径折算用），年末周点（2024-12-30 当周）无
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2024-03-04"]);
    seed_price_history(&conn, "ph-fx", "inst-fx", "2024-12-31", 1_000_000, "USD");

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-fx",
            "inst-fx",
            10.0,
            1_000_000,
            "2024-03-06",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-fx", "inst-fx", 10_000, "CNY", "2024-05-01"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_year.len(), 1);
    assert_eq!(result.by_year[0].unrealized_change_cents, None);
    assert_eq!(result.by_year[0].annual_return_cents, None);
}

#[test]
fn annual_return_computable_when_dividend_lands_outside_investment_accounts() {
    // 分红只到账非投资账户（该币种无任何投资账户）→ 组内无持仓、市值腿为 0，
    // 该年可算（年度收益 = 分红）——不特判会被误判为不可算（ADR-0132 空值语义
    // 只辖「有持仓而缺料」，不辖「无持仓」）。
    let conn = open();
    seed_account(&conn, "acc-bank", "银行卡", "cash", "CNY", 0);
    seed_instrument(&conn, "inst-bk", "501000", "基金A", "CNY", "unknown");

    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-bank", "inst-bk", 88_000, "CNY", "2021-12-31"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_year.len(), 1);
    assert_eq!(result.by_year[0].unrealized_change_cents, Some(0));
    assert_eq!(result.by_year[0].annual_return_cents, Some(88_000));
}

#[test]
fn annual_return_constant_price_instrument_is_computable() {
    // 恒定价格标的（货基，ADR-0126）不落价格历史行，边界市值由常量在装载器内
    // 覆盖：持仓跨年、库内无任何价格历史 → 该年可算、未实现变动 0（份额不变、
    // 价格恒定），年度收益 = 分红腿——「老婆的且慢」2019 行（当年只持货基）的
    // 货基形态；常量覆盖失效时该行会被误判为不可算，本测试钉住该路径。
    let conn = open();
    seed_account(&conn, "acc-mm", "且慢", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-mm", "000198", "货币基金", "CNY", "unknown");
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = 'inst-mm'",
        [],
    )
    .unwrap();

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-mm",
            "inst-mm",
            10.0,
            10_000,
            "2019-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_dividend_input_on("acc-mm", "inst-mm", 836_536, "CNY", "2019-12-31"),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.by_year.len(), 1);
    let row = &result.by_year[0];
    assert_eq!(row.year, "2019");
    // 期末市值 = 10 份 × 恒定 1.0000 元 = 10 元，买入支出同值 → 未实现变动 0
    assert_eq!(row.unrealized_change_cents, Some(0));
    assert_eq!(row.annual_return_cents, Some(836_536));
}
