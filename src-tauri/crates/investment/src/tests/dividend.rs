use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{
    TransactionInput, create_transaction_internal, delete_transaction_internal,
    update_transaction_internal,
};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_fx_history_weeks, seed_instrument};

fn balance_of(conn: &rusqlite::Connection, account_id: &str) -> i64 {
    ledger_accounts::balance::compute_balance(conn, account_id).unwrap()
}

type SecurityRow = (String, String, Option<f64>, Option<i64>, i64);

fn security_row(conn: &rusqlite::Connection, transaction_id: &str) -> Option<SecurityRow> {
    conn.query_row(
        "SELECT instrument_id, action, quantity, price_cents, fee_cents \
         FROM security_transactions WHERE transaction_id=?1",
        rusqlite::params![transaction_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )
    .ok()
}

fn seed_dividend_scene(conn: &rusqlite::Connection) {
    seed_account(conn, "acc-dv", "股票户", "investment", "CNY", 0);
    seed_account(conn, "acc-dv-cash", "银行卡", "bank", "CNY", 0);
    seed_instrument(conn, "inst-dv", "502010", "证券基金", "CNY", "unknown");
}

fn make_income_input(account_id: &str, amount_cents: i64, currency: &str) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Income,
        amount_cents,
        currency_code: currency.into(),
        account_id: Some(account_id.into()),
        funding: Vec::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-02-11".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

#[test]
fn dividend_create_records_cash_leg_and_instrument_extension() {
    let conn = open();
    seed_dividend_scene(&conn);

    let write =
        create_transaction_internal(&conn, make_dividend_input("acc-dv", "inst-dv", 3000, "CNY"))
            .unwrap();

    let row: (String, i64, String, i64) = conn
        .query_row(
            "SELECT kind, amount_cents, currency_code, amount_native_cents \
             FROM transactions WHERE id=?1",
            rusqlite::params![write.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(row, ("dividend".to_string(), 3000, "CNY".to_string(), 3000));

    assert_eq!(
        security_row(&conn, &write.id),
        Some(("inst-dv".to_string(), "dividend".to_string(), None, None, 0))
    );
    assert_eq!(balance_of(&conn, "acc-dv"), 3000);
    assert!(list_holdings(&conn).unwrap().is_empty());
    assert_eq!(
        query_realized_pnl_summary(
            &conn,
            &PnlFilter {
                account_id: None,
                instrument_id: None,
            }
        )
        .unwrap()
        .by_year
        .iter()
        .map(|r| r.realized_pnl_cents.unwrap_or(0))
        .sum::<i64>(),
        0
    );
    let sources =
        source_display_by_transaction_ids(&conn, std::slice::from_ref(&write.id)).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].instrument_id, "inst-dv");
}

#[test]
fn dividend_can_land_in_any_active_account() {
    let conn = open();
    seed_dividend_scene(&conn);

    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv-cash", "inst-dv", 1500, "CNY"),
    )
    .unwrap();
    assert_eq!(balance_of(&conn, "acc-dv-cash"), 1500);
}

#[test]
fn dividend_create_guards_return_coded_errors() {
    let conn = open();
    seed_dividend_scene(&conn);
    seed_account(&conn, "acc-dv-usd", "美股账户", "investment", "USD", 0);

    let err = |r: ledger_infra::error::Result<ledger_transaction::TransactionWrite>| {
        r.expect_err("应被拒绝")
    };

    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.instrument_id = None;
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-instrument-required");

    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv", "inst-missing", 3000, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-instrument-not-found");

    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv", "inst-dv", 0, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-amount-positive");
    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv", "inst-dv", -100, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-amount-positive");

    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-missing", "inst-dv", 3000, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-account-not-found");

    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv-usd", "inst-dv", 3000, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-currency-mismatch");

    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.quantity = Some(1.0);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-quantity-forbidden");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.price_cents = Some(100);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-price-forbidden");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.fee_cents = Some(1);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-fee-forbidden");

    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.to_instrument_id = Some("inst-other".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-to-instrument-forbidden");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.to_account_id = Some("acc-dv-cash".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-to-account-forbidden");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.funding_account_id = Some("acc-dv-cash".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.funding-unsupported");

    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.merchant_id = Some("m-1".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.merchant-unsupported");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.category_id = Some("c-1".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.category-unsupported");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.policy_id = Some("p-1".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.policy-unsupported");
}

#[test]
fn dividend_update_replaces_fields_and_delete_removes_extension() {
    let conn = open();
    seed_dividend_scene(&conn);
    let id =
        create_transaction_internal(&conn, make_dividend_input("acc-dv", "inst-dv", 3000, "CNY"))
            .unwrap()
            .id;

    update_transaction_internal(
        &conn,
        &id,
        make_dividend_input("acc-dv", "inst-dv", 5000, "CNY"),
    )
    .unwrap();
    let amount: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount, 5000);
    assert_eq!(balance_of(&conn, "acc-dv"), 5000);
    assert_eq!(
        security_row(&conn, &id),
        Some(("inst-dv".to_string(), "dividend".to_string(), None, None, 0))
    );

    delete_transaction_internal(&conn, &id).unwrap();
    assert_eq!(security_row(&conn, &id), None);
    assert_eq!(balance_of(&conn, "acc-dv"), 0);
}

#[test]
fn dividend_kind_change_is_forbidden_in_both_directions() {
    let conn = open();
    seed_dividend_scene(&conn);
    let dividend_id =
        create_transaction_internal(&conn, make_dividend_input("acc-dv", "inst-dv", 3000, "CNY"))
            .unwrap()
            .id;
    let income_id = create_transaction_internal(&conn, make_income_input("acc-dv", 1000, "CNY"))
        .unwrap()
        .id;

    let e = update_transaction_internal(
        &conn,
        &dividend_id,
        make_income_input("acc-dv", 1000, "CNY"),
    )
    .expect_err("改出 dividend 应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.dividend-kind-change-forbidden");

    let e = update_transaction_internal(
        &conn,
        &income_id,
        make_dividend_input("acc-dv", "inst-dv", 3000, "CNY"),
    )
    .expect_err("改入 dividend 应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.dividend-kind-change-forbidden");
}

#[test]
fn dividend_native_cents_converted_via_trade_date_history() {
    let conn = open();
    seed_account(&conn, "acc-dv-usd", "美股户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-dv-usd", "AAPL", "苹果", "USD", "unknown");
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.2, &["2026-02-10"]);

    let write = create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv-usd", "inst-dv-usd", 3000, "USD"),
    )
    .unwrap();

    let (amount_cents, amount_native_cents, fx_rate_used, fx_rate_source): (
        i64,
        i64,
        Option<f64>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents, fx_rate_used, fx_rate_source \
             FROM transactions WHERE id=?1",
            rusqlite::params![write.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 3000, "现金腿 = 分红金额（USD 分）");
    assert_eq!(
        amount_native_cents, 21_600,
        "本位币折算按交易日历史（3000 × 7.2）——改回当期入口本断言即红"
    );
    assert_eq!(fx_rate_used, Some(7.2), "折算留痕随行");
    assert_eq!(fx_rate_source.as_deref(), Some("series"), "来源 = 序列命中");
}
