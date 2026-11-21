use rusqlite::Connection;

use super::super::{OpOutcome, parked_ops, read_ops};
use super::common::{read_lot, read_lot_sale, read_transaction, seed_device, wire_in, wire_out};
use ledger_accounts::balance::cached_balance;
use ledger_infra::db;
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
        seed_account(conn, "acc-cv", "基金户", "investment", "CNY", 0);
        seed_instrument(conn, "inst-out", "006793", "转出基金", "CNY", "unknown");
        seed_instrument(conn, "inst-in", "519700", "转入基金", "CNY", "unknown");
        ledger_accounts::balance::refresh_all_account_balances(conn).unwrap();
    }
    (conn_a, conn_b)
}

fn buy_input(
    account_id: &str,
    instrument_id: &str,
    quantity: f64,
    price_cents: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(quantity),
        price_cents: Some(price_cents),
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

fn sell_input(
    account_id: &str,
    instrument_id: &str,
    quantity: f64,
    price_cents: i64,
) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Sell,
        date: "2026-03-01".into(),
        ..buy_input(
            account_id,
            instrument_id,
            quantity,
            price_cents,
            "2026-03-01",
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn convert_input(
    account_id: &str,
    instrument_id: &str,
    to_instrument_id: &str,
    quantity: f64,
    to_quantity: f64,
    out_amount_cents: i64,
    in_amount_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Convert,
        quantity: Some(quantity),
        to_instrument_id: Some(to_instrument_id.into()),
        to_quantity: Some(to_quantity),
        out_amount_cents: Some(out_amount_cents),
        in_amount_cents: Some(in_amount_cents),
        fee_cents: Some(fee_cents),
        date: "2026-02-01".into(),
        ..buy_input(account_id, instrument_id, quantity, 0, "2026-02-01")
    }
}

fn read_conversions(conn: &Connection, convert_id: &str) -> Vec<(String, f64, i64, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT l.buy_transaction_id, c.quantity, c.cost_per_unit_cents, c.cost_cents \
             FROM security_lot_conversions c JOIN security_lots l ON l.id = c.lot_id \
             WHERE c.transaction_id = ?1 ORDER BY l.buy_transaction_id",
        )
        .unwrap();
    stmt.query_map([convert_id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

#[allow(clippy::type_complexity)]
fn read_convert_row(conn: &Connection, id: &str) -> Option<(f64, i64, i64, String, f64, i64, i64)> {
    conn.query_row(
        "SELECT quantity, price_cents, fee_cents, to_instrument_id, to_quantity, \
         out_amount_cents, in_amount_cents FROM security_transactions \
         WHERE transaction_id = ?1 AND action = 'convert'",
        [id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        },
    )
    .ok()
}

fn assert_full_redelivery_is_idempotent(conn_a: &Connection, conn_b: &Connection) {
    let reports = wire_in(conn_b, &wire_out(conn_a));
    assert!(
        reports.iter().all(|r| r.outcome == OpOutcome::Skipped),
        "重投递应全部幂等跳过：{reports:?}"
    );
    assert_eq!(read_ops(conn_a).unwrap(), read_ops(conn_b).unwrap());
}

#[test]
fn convert_create_replay_converges_lots_carried_cost_and_pnl() {
    let (conn_a, conn_b) = seed_both_ends();
    let buy1 = protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 10_000, "2026-01-10"),
    )
    .unwrap()
    .id;
    let buy2 = protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 20_000, "2026-01-11"),
    )
    .unwrap()
    .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    let convert_id = protocol::create(
        &conn_a,
        convert_input("acc-cv", "inst-out", "inst-in", 15.0, 15.0, 2_250, 2_250, 0),
    )
    .unwrap()
    .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    let expected_row = read_transaction(&conn_a, &convert_id).unwrap();
    assert_eq!(expected_row.kind, TransactionKind::Convert);
    assert_eq!(expected_row.amount_cents, 2_000, "行锚点 = FIFO 结转成本");
    assert_eq!(
        read_transaction(&conn_b, &convert_id).unwrap(),
        expected_row,
        "转换行重放后一致"
    );
    assert_eq!(
        read_convert_row(&conn_a, &convert_id),
        read_convert_row(&conn_b, &convert_id)
    );
    assert_eq!(
        read_convert_row(&conn_a, &convert_id),
        Some((15.0, 15_000, 0, "inst-in".into(), 15.0, 2_250, 2_250))
    );

    let mut expected_conversions = vec![
        (buy1.clone(), 10.0, 10_000, 1_000),
        (buy2.clone(), 5.0, 20_000, 1_000),
    ];
    expected_conversions.sort_by(|a, b| a.0.cmp(&b.0));
    let mut conversions_a = read_conversions(&conn_a, &convert_id);
    conversions_a.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(conversions_a, expected_conversions);
    let mut conversions_b = read_conversions(&conn_b, &convert_id);
    conversions_b.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        conversions_b, expected_conversions,
        "本地重建 FIFO 快照 ⇒ 同一逐批次消耗与结转成本"
    );

    assert_eq!(
        read_lot(&conn_a, &buy1),
        Some((10.0, 0.0, 10_000, "CNY".into()))
    );
    assert_eq!(
        read_lot(&conn_a, &buy2),
        Some((10.0, 5.0, 20_000, "CNY".into()))
    );
    assert_eq!(
        read_lot(&conn_a, &convert_id),
        Some((15.0, 15.0, 13_333, "CNY".into())),
        "转入批次以结转成本建仓"
    );
    for anchor in [&buy1, &buy2, &convert_id] {
        assert_eq!(
            read_lot(&conn_b, anchor),
            read_lot(&conn_a, anchor),
            "批次重放后一致：{anchor}"
        );
    }

    let sell_id = protocol::create(&conn_a, sell_input("acc-cv", "inst-in", 5.0, 30_000))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(
        read_lot_sale(&conn_a, &sell_id, &convert_id),
        Some((5.0, 13_333, 833)),
        "结转成本流入已实现盈亏"
    );
    assert_eq!(
        read_lot_sale(&conn_b, &sell_id, &convert_id),
        read_lot_sale(&conn_a, &sell_id, &convert_id),
        "盈亏重放后一致"
    );

    assert_eq!(cached_balance(&conn_a, "acc-cv").unwrap(), -3_000 + 1_500);
    assert_eq!(
        cached_balance(&conn_b, "acc-cv").unwrap(),
        cached_balance(&conn_a, "acc-cv").unwrap()
    );
    assert_balance_cache_matches_realtime(&conn_b);

    assert_full_redelivery_is_idempotent(&conn_a, &conn_b);
}

#[test]
fn convert_update_replay_rebuilds_both_legs() {
    let (conn_a, conn_b) = seed_both_ends();
    let buy_id = protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 10_000, "2026-01-10"),
    )
    .unwrap()
    .id;
    let convert_id = protocol::create(
        &conn_a,
        convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(
        read_lot(&conn_b, &buy_id),
        Some((10.0, 0.0, 10_000, "CNY".into()))
    );

    protocol::update(
        &conn_a,
        &convert_id,
        convert_input("acc-cv", "inst-out", "inst-in", 5.0, 5.0, 550, 550, 0),
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));

    assert_eq!(
        read_transaction(&conn_a, &convert_id).unwrap().amount_cents,
        500
    );
    assert_eq!(
        read_transaction(&conn_b, &convert_id).unwrap(),
        read_transaction(&conn_a, &convert_id).unwrap()
    );
    assert_eq!(
        read_lot(&conn_a, &buy_id),
        Some((10.0, 5.0, 10_000, "CNY".into()))
    );
    assert_eq!(
        read_lot(&conn_a, &convert_id),
        Some((5.0, 5.0, 10_000, "CNY".into()))
    );
    assert_eq!(
        read_conversions(&conn_a, &convert_id),
        vec![(buy_id.clone(), 5.0, 10_000, 500)]
    );
    for anchor in [&buy_id, &convert_id] {
        assert_eq!(
            read_lot(&conn_b, anchor),
            read_lot(&conn_a, anchor),
            "批次两端一致"
        );
    }
    assert_eq!(
        read_conversions(&conn_b, &convert_id),
        read_conversions(&conn_a, &convert_id)
    );
    assert_balance_cache_matches_realtime(&conn_b);

    assert_full_redelivery_is_idempotent(&conn_a, &conn_b);
}

#[test]
fn convert_delete_replay_reverses_both_legs() {
    let (conn_a, conn_b) = seed_both_ends();
    let buy_id = protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 10_000, "2026-01-10"),
    )
    .unwrap()
    .id;
    let convert_id = protocol::create(
        &conn_a,
        convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    protocol::delete(&conn_a, &convert_id).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));

    let deleted = |conn: &Connection| read_transaction(conn, &convert_id).unwrap().is_deleted;
    assert_eq!(deleted(&conn_a), 1);
    assert_eq!(deleted(&conn_b), 1, "删除重放后同软删");
    for conn in [&conn_a, &conn_b] {
        assert_eq!(
            read_lot(conn, &buy_id),
            Some((10.0, 10.0, 10_000, "CNY".into()))
        );
        assert_eq!(read_lot(conn, &convert_id), None, "转入批次清除");
        assert!(
            read_conversions(conn, &convert_id).is_empty(),
            "转出消耗记录清除"
        );
    }
    assert_balance_cache_matches_realtime(&conn_b);

    assert_full_redelivery_is_idempotent(&conn_a, &conn_b);
}

#[test]
fn convert_before_buy_parks_then_redelivery_self_heals() {
    let (conn_a, conn_b) = seed_both_ends();
    let buy_id = protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 10_000, "2026-01-10"),
    )
    .unwrap()
    .id;
    let convert_id = protocol::create(
        &conn_a,
        convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    let wire = wire_out(&conn_a);
    assert_eq!(wire.len(), 2, "两笔交易各一条 op");
    let (buy_wire, convert_wire) = (vec![wire[0].clone()], vec![wire[1].clone()]);

    let reports = wire_in(&conn_b, &convert_wire);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "trade.insufficient-holding"),
        "可卖数量不足挂起并携带既有码：{:?}",
        reports[0]
    );
    assert!(read_ops(&conn_b).unwrap().is_empty(), "挂起不落日志");
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);

    wire_in(&conn_b, &buy_wire);
    assert_eq!(
        parked_ops(&conn_b).unwrap().len(),
        1,
        "buy 补达不自动解决挂起"
    );
    let reports = wire_in(&conn_b, &convert_wire);
    assert_eq!(reports[0].outcome, OpOutcome::Applied);
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert_eq!(read_lot(&conn_b, &buy_id), read_lot(&conn_a, &buy_id));
    assert_eq!(
        read_lot(&conn_b, &convert_id),
        read_lot(&conn_a, &convert_id)
    );
    assert_eq!(
        read_conversions(&conn_b, &convert_id),
        read_conversions(&conn_a, &convert_id)
    );
}

#[test]
fn convert_op_schema_ahead_parks_then_retry_after_upgrade_succeeds() {
    let (conn_a, conn_b) = seed_both_ends();
    protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 10_000, "2026-01-10"),
    )
    .unwrap();
    let convert_id = protocol::create(
        &conn_a,
        convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    let wire = wire_out(&conn_a);
    wire_in(&conn_b, &[wire[0].clone()]);

    let local_version = db::schema_version(&conn_b).unwrap();
    let mut ahead: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    ahead["schema_version"] = serde_json::json!(local_version + 1);
    let reports = wire_in(&conn_b, &[serde_json::to_string(&ahead).unwrap()]);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "sync-engine.schema-ahead"),
        "schema 超前挂起并携带升级提示码：{:?}",
        reports[0]
    );
    assert!(
        read_transaction(&conn_b, &convert_id).is_none(),
        "未执行不落地"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1, "挂起行可查");

    ahead["schema_version"] = serde_json::json!(local_version);
    let reports = wire_in(&conn_b, &[serde_json::to_string(&ahead).unwrap()]);
    assert_eq!(reports[0].outcome, OpOutcome::Applied);
    assert!(
        parked_ops(&conn_b).unwrap().is_empty(),
        "升级后重试成功即出队"
    );
    assert_eq!(
        read_transaction(&conn_b, &convert_id).unwrap(),
        read_transaction(&conn_a, &convert_id).unwrap()
    );
    assert_eq!(
        read_lot(&conn_b, &convert_id),
        read_lot(&conn_a, &convert_id)
    );
    assert_eq!(
        read_conversions(&conn_b, &convert_id),
        read_conversions(&conn_a, &convert_id)
    );
}

#[test]
fn legacy_op_without_convert_member_replays_with_default_semantics() {
    let (conn_a, conn_b) = seed_both_ends();
    let buy_id = protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 10_000, "2026-01-10"),
    )
    .unwrap()
    .id;

    let mut legacy: serde_json::Value = serde_json::from_str(&wire_out(&conn_a)[0]).unwrap();
    let removed = legacy["command"]["payload"]
        .as_object_mut()
        .unwrap()
        .remove("convert")
        .is_some();
    assert!(removed, "前置：op 载荷应携带 convert 成员");
    let raw = serde_json::to_string(&legacy).unwrap();
    let reports = wire_in(&conn_b, std::slice::from_ref(&raw));
    assert!(
        matches!(reports[0].outcome, OpOutcome::Applied),
        "旧格式 op 应照常应用，实际: {:?}",
        reports[0].outcome
    );
    assert_eq!(read_lot(&conn_b, &buy_id), read_lot(&conn_a, &buy_id));

    let again = wire_in(&conn_b, &[raw]);
    assert_eq!(again[0].outcome, OpOutcome::Skipped);
    assert!(parked_ops(&conn_b).unwrap().is_empty());
}

#[test]
fn legacy_convert_op_without_fields_parks_without_booking() {
    let (conn_a, conn_b) = seed_both_ends();
    protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 10_000, "2026-01-10"),
    )
    .unwrap();
    let convert_id = protocol::create(
        &conn_a,
        convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    let wire = wire_out(&conn_a);
    wire_in(&conn_b, &[wire[0].clone()]);

    let mut legacy: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    legacy["command"]["payload"]
        .as_object_mut()
        .unwrap()
        .remove("convert");
    let reports = wire_in(&conn_b, &[serde_json::to_string(&legacy).unwrap()]);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "transaction.convert-fields-missing"),
        "缺转换字段的转换 op 应码化挂起，实际: {:?}",
        reports[0]
    );
    assert!(
        read_transaction(&conn_b, &convert_id).is_none(),
        "不落转换行"
    );
    assert_eq!(read_lot(&conn_b, &convert_id), None, "不落转入批次");
    assert!(
        read_conversions(&conn_b, &convert_id).is_empty(),
        "不落转出消耗"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);
}

#[test]
fn convert_op_with_divergent_carried_cost_parks_without_booking() {
    let (conn_a, conn_b) = seed_both_ends();
    protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 10_000, "2026-01-10"),
    )
    .unwrap();
    let convert_id = protocol::create(
        &conn_a,
        convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    let wire = wire_out(&conn_a);
    wire_in(&conn_b, &[wire[0].clone()]);

    let mut tampered: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    tampered["command"]["payload"]["convert"]["carried_cost_cents"] = serde_json::json!(999);
    let reports = wire_in(&conn_b, &[serde_json::to_string(&tampered).unwrap()]);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "transaction.convert-carried-cost-mismatch"),
        "结转成本发散应码化挂起，实际: {:?}",
        reports[0]
    );
    assert!(
        read_transaction(&conn_b, &convert_id).is_none(),
        "不落转换行"
    );
    assert_eq!(read_lot(&conn_b, &convert_id), None, "不落转入批次");
    assert!(
        read_conversions(&conn_b, &convert_id).is_empty(),
        "不落转出消耗"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);
}

#[test]
fn convert_op_with_to_account_parks_without_booking() {
    let (conn_a, conn_b) = seed_both_ends();
    protocol::create(
        &conn_a,
        buy_input("acc-cv", "inst-out", 10.0, 10_000, "2026-01-10"),
    )
    .unwrap();
    let convert_id = protocol::create(
        &conn_a,
        convert_input("acc-cv", "inst-out", "inst-in", 10.0, 10.0, 1_100, 1_100, 0),
    )
    .unwrap()
    .id;
    let wire = wire_out(&conn_a);
    wire_in(&conn_b, &[wire[0].clone()]);

    let mut tampered: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    tampered["command"]["payload"]["row"]["to_account_id"] = serde_json::json!("acc-cv");
    let reports = wire_in(&conn_b, &[serde_json::to_string(&tampered).unwrap()]);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "trade.convert-to-account-forbidden"),
        "带转入账户的转换 op 应码化挂起，实际: {:?}",
        reports[0]
    );
    assert!(
        read_transaction(&conn_b, &convert_id).is_none(),
        "不落转换行"
    );
    assert_eq!(read_lot(&conn_b, &convert_id), None, "不落转入批次");
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);
}
