//! 已实现盈亏汇总（realized PnL）测试：空态、单笔 / 多账户聚合、按账户 / 按
//! 标的过滤（issue #257 纯移动归组）。

use crate::transaction::create_transaction_internal;

use super::super::*;
use super::common::*;
use crate::test_support::{open, seed_account, seed_exchange_rate, seed_instrument};

fn empty_filter() -> PnlFilter {
    PnlFilter {
        account_id: None,
        instrument_id: None,
    }
}

#[test]
fn realized_pnl_summary_empty_when_no_sales() {
    let conn = open();
    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.total_realized_pnl_cents, 0);
    assert!(result.by_year.is_empty());
    assert!(result.by_account.is_empty());
    assert!(result.by_instrument.is_empty());
    assert!(result.details.is_empty());
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

    assert_eq!(result.total_realized_pnl_cents, 9800);
    assert_eq!(result.by_year.len(), 1);
    assert_eq!(result.by_year[0].realized_pnl_cents, 9800);
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-pnl");
    assert_eq!(result.by_account[0].realized_pnl_cents, 9800);
    assert_eq!(result.by_instrument.len(), 1);
    assert_eq!(result.by_instrument[0].instrument_id, "inst-pnl");
    assert_eq!(result.by_instrument[0].symbol, "AAPL");
    assert_eq!(result.by_instrument[0].realized_pnl_cents, 9800);
    assert_eq!(result.details.len(), 1);
    assert_eq!(result.details[0].instrument_symbol, "AAPL");
    assert_eq!(result.details[0].quantity, 5.0);
    assert_eq!(result.details[0].realized_pnl_cents, 9800);
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

    assert_eq!(result.total_realized_pnl_cents, 3000);
    assert_eq!(result.by_account.len(), 2);
    assert_eq!(result.by_account[0].account_id, "acc-a");
    assert_eq!(result.by_account[0].realized_pnl_cents, 2000);
    assert_eq!(result.by_account[1].account_id, "acc-b");
    assert_eq!(result.by_account[1].realized_pnl_cents, 1000);
    assert_eq!(result.details.len(), 2);
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

    assert_eq!(result.total_realized_pnl_cents, 2000);
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.details.len(), 1);
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
    assert_eq!(result.total_realized_pnl_cents, 2000);
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-live");
    assert_eq!(result.details.len(), 1);
    assert_eq!(result.details[0].account_name, "在用户");
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
    assert_eq!(baseline.total_realized_pnl_cents, 2000);

    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        rusqlite::params![sell_id],
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.total_realized_pnl_cents, 0);
    assert!(result.by_year.is_empty());
    assert!(result.by_account.is_empty());
    assert!(result.by_instrument.is_empty());
    assert!(result.details.is_empty());
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

    assert_eq!(result.total_realized_pnl_cents, 2000);
    assert_eq!(result.by_instrument.len(), 1);
    assert_eq!(result.by_instrument[0].instrument_id, "inst-a");
    assert_eq!(result.details.len(), 1);
}
