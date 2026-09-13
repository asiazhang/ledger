//! 已实现盈亏汇总（realized PnL）测试：空态、单笔 / 多账户聚合、按账户 / 按
//! 标的过滤、按币种分组不混算（ADR-0107；issue #257 纯移动归组）。

use ledger_transaction::create_transaction_internal;

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_exchange_rate, seed_instrument};

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
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
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
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-pnl");
    assert_eq!(result.by_account[0].currency_code, "USD");
    assert_eq!(result.by_account[0].realized_pnl_cents, 9800);
    assert_eq!(result.by_instrument.len(), 1);
    assert_eq!(result.by_instrument[0].instrument_id, "inst-pnl");
    assert_eq!(result.by_instrument[0].symbol, "AAPL");
    assert_eq!(result.by_instrument[0].realized_pnl_cents, 9800);
}

#[test]
fn realized_pnl_summary_aggregates_multiple_accounts() {
    let conn = open();
    seed_account(&conn, "acc-a", "账户A", "investment", "USD", 0);
    seed_account(&conn, "acc-b", "账户B", "investment", "USD", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
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
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
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
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
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
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
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
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
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
    seed_exchange_rate(&conn, "USD", "CNY", 1.0);
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
