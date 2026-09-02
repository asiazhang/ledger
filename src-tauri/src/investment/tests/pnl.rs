//! 已实现盈亏汇总（realized PnL）测试：空态、单笔 / 多账户聚合、按账户 / 按
//! 标的过滤（issue #257 纯移动归组）。

use crate::transaction::create_transaction_internal;

use super::super::*;
use super::common::*;

fn empty_filter() -> PnlFilter {
    PnlFilter {
        account_id: None,
        instrument_id: None,
    }
}

#[test]
fn realized_pnl_summary_empty_when_no_sales() {
    let conn = setup_db();
    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.total_realized_pnl_cents, 0);
    assert!(result.by_year.is_empty());
    assert!(result.by_account.is_empty());
    assert!(result.by_instrument.is_empty());
    assert!(result.details.is_empty());
}

#[test]
fn realized_pnl_summary_aggregates_single_sale() {
    let conn = setup_db();
    insert_account(&conn, "acc-pnl", "美股账户", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-pnl", "AAPL", "Apple", "USD");

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
    let conn = setup_db();
    insert_account(&conn, "acc-a", "账户A", "investment", "USD");
    insert_account(&conn, "acc-b", "账户B", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD");

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
    let conn = setup_db();
    insert_account(&conn, "acc-a", "账户A", "investment", "USD");
    insert_account(&conn, "acc-b", "账户B", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD");

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
fn realized_pnl_summary_filter_by_instrument() {
    let conn = setup_db();
    insert_account(&conn, "acc-pnl", "美股", "investment", "USD");
    insert_rate_1_1(&conn, "USD");
    insert_instrument(&conn, "inst-a", "AAPL", "Apple", "USD");
    insert_instrument(&conn, "inst-b", "GOOGL", "Alphabet", "USD");

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
