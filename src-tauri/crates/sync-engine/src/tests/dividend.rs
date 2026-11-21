use rusqlite::Connection;

use super::super::{OpOutcome, parked_ops, read_ops};
use super::common::{read_transaction, seed_device, wire_in, wire_out};
use ledger_accounts::balance::cached_balance;
use ledger_investment::query_cumulative_pnl_summary;
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support::{
    self, assert_balance_cache_matches_realtime, seed_account, seed_instrument,
};

fn seed_both_ends() -> (Connection, Connection) {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-dv", "股票户", "investment", "CNY", 0);
        seed_instrument(conn, "inst-dv", "502010", "证券基金", "CNY", "unknown");
        ledger_accounts::balance::refresh_all_account_balances(conn).unwrap();
    }
    (conn_a, conn_b)
}

fn dividend_input(
    account_id: &str,
    instrument_id: &str,
    amount_cents: i64,
    note: &str,
) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Dividend,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: Some(note.into()),
        date: "2026-02-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: None,
        price_cents: None,
        fee_cents: Some(0),
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

fn dividend_extension(
    conn: &Connection,
    tx_id: &str,
) -> Option<(String, Option<f64>, Option<i64>)> {
    conn.query_row(
        "SELECT instrument_id, quantity, price_cents FROM security_transactions \
         WHERE transaction_id=?1 AND action='dividend'",
        [tx_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

fn cumulative_of(conn: &Connection, currency: &str) -> i64 {
    query_cumulative_pnl_summary(conn)
        .unwrap()
        .iter()
        .find(|g| g.currency_code == currency)
        .map(|g| g.cumulative_pnl_cents)
        .unwrap_or(0)
}

fn assert_dividend_converged(conn_a: &Connection, conn_b: &Connection, tx_id: &str) {
    assert_eq!(
        read_transaction(conn_b, tx_id),
        read_transaction(conn_a, tx_id),
        "交易行重放后一致"
    );
    assert_eq!(
        dividend_extension(conn_b, tx_id),
        dividend_extension(conn_a, tx_id),
        "dividend 扩展行重放后一致"
    );
    assert_eq!(
        cached_balance(conn_b, "acc-dv").unwrap(),
        cached_balance(conn_a, "acc-dv").unwrap(),
        "余额缓存重放后一致"
    );
    assert_eq!(
        cumulative_of(conn_b, "CNY"),
        cumulative_of(conn_a, "CNY"),
        "累计收益第三腿重放后一致"
    );
    assert_balance_cache_matches_realtime(conn_b);
}

#[test]
fn dividend_create_update_delete_replay_converges() {
    let (conn_a, conn_b) = seed_both_ends();
    let tx_id = protocol::create(&conn_a, dividend_input("acc-dv", "inst-dv", 3000, "现场A"))
        .unwrap()
        .id;

    let wire = wire_out(&conn_a);
    let op: serde_json::Value = serde_json::from_str(wire.last().unwrap()).unwrap();
    assert_eq!(
        op["command"]["payload"]["investment"]["instrument_id"],
        serde_json::json!("inst-dv")
    );
    assert_eq!(
        op["command"]["payload"]["row"]["amount_cents"],
        serde_json::json!(3000),
        "现金腿随行携带"
    );

    wire_in(&conn_b, &wire);
    assert_dividend_converged(&conn_a, &conn_b, &tx_id);
    assert_eq!(cached_balance(&conn_a, "acc-dv").unwrap(), 3000);
    assert_eq!(
        dividend_extension(&conn_a, &tx_id),
        Some(("inst-dv".to_string(), None, None))
    );

    protocol::update(
        &conn_a,
        &tx_id,
        dividend_input("acc-dv", "inst-dv", 5000, "现场B"),
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_dividend_converged(&conn_a, &conn_b, &tx_id);
    assert_eq!(cached_balance(&conn_a, "acc-dv").unwrap(), 5000);
    assert_eq!(cumulative_of(&conn_a, "CNY"), 5000);

    protocol::delete(&conn_a, &tx_id).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_dividend_converged(&conn_a, &conn_b, &tx_id);
    assert_eq!(read_transaction(&conn_a, &tx_id).unwrap().is_deleted, 1);
    assert_eq!(read_transaction(&conn_b, &tx_id).unwrap().is_deleted, 1);
    assert_eq!(dividend_extension(&conn_a, &tx_id), None);
    assert_eq!(dividend_extension(&conn_b, &tx_id), None);
    assert_eq!(cached_balance(&conn_a, "acc-dv").unwrap(), 0);
    assert_eq!(cached_balance(&conn_b, "acc-dv").unwrap(), 0);

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        reports.iter().all(|r| r.outcome == OpOutcome::Skipped),
        "重投递应全部幂等跳过：{reports:?}"
    );
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn dividend_dependency_missing_parks_then_redelivery_self_heals() {
    let (conn_a, conn_b) = seed_both_ends();
    conn_b
        .execute("DELETE FROM instruments WHERE id='inst-dv'", [])
        .unwrap();
    let tx_id = protocol::create(&conn_a, dividend_input("acc-dv", "inst-dv", 3000, "现场A"))
        .unwrap()
        .id;
    let wire = wire_out(&conn_a);

    let reports = wire_in(&conn_b, &wire);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "trade.dividend-instrument-not-found"),
        "标的不在应挂起并携带既有码：{:?}",
        reports[0]
    );
    assert!(read_ops(&conn_b).unwrap().is_empty(), "挂起不落日志");
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);
    assert!(read_transaction(&conn_b, &tx_id).is_none(), "不落分红行");
    assert!(dividend_extension(&conn_b, &tx_id).is_none());

    seed_instrument(&conn_b, "inst-dv", "502010", "证券基金", "CNY", "unknown");
    let reports = wire_in(&conn_b, &wire);
    assert_eq!(reports[0].outcome, OpOutcome::Applied);
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert_dividend_converged(&conn_a, &conn_b, &tx_id);
}

#[test]
fn dividend_currency_divergence_parks_without_booking() {
    let (conn_a, conn_b) = seed_both_ends();
    let tx_id = protocol::create(&conn_a, dividend_input("acc-dv", "inst-dv", 3000, "现场A"))
        .unwrap()
        .id;
    let wire = wire_out(&conn_a);

    let mut tampered: serde_json::Value = serde_json::from_str(&wire[0]).unwrap();
    tampered["command"]["payload"]["row"]["currency_code"] = serde_json::json!("USD");
    let reports = wire_in(&conn_b, &[serde_json::to_string(&tampered).unwrap()]);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "trade.dividend-currency-mismatch"),
        "币种不一致应码化挂起，实际: {:?}",
        reports[0]
    );
    assert!(read_ops(&conn_b).unwrap().is_empty(), "挂起不落日志");
    assert!(read_transaction(&conn_b, &tx_id).is_none(), "不落分红行");
}
