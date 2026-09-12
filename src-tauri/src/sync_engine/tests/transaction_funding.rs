//! 出资账户的同步重放收敛（issue #939 / ADR-0096 决策 8）：创建/修改/删除
//! 重放后两端余额一致（三端归因自动覆盖）；旧格式 op（行载荷无出资字段，
//! V023 前设备产出）重放不报错、按旧语义落库（现金腿记投资账户）。
//!
//! `NormalizedTransaction` 随 op 携带出资账户（#935 接线），本模块按验收
//! 钉住行为：余额一致、旧载荷前向兼容。

use super::super::{OpOutcome, apply_ops, parked_ops, read_ops};
use super::common::{read_transaction, seed_device, wire_in, wire_out};
use crate::accounts::balance::cached_balance;
use crate::sync_engine::ingest_ops;
use crate::test_support::{
    self, assert_balance_cache_matches_realtime, seed_account, seed_investment_setup,
};
use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::write::protocol;

fn buy_input(account_id: &str, instrument_id: &str, funding: Option<&str>) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: funding.map(Into::into),
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(10.0),
        price_cents: Some(1500),
        fee_cents: Some(5),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}

fn sell_input(account_id: &str, instrument_id: &str, funding: Option<&str>) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Sell,
        quantity: Some(4.0),
        fee_cents: Some(0),
        funding_account_id: funding.map(Into::into),
        ..buy_input(account_id, instrument_id, None)
    }
}

/// 双端铺垫：投资账户 + 标的 + 两个 USD 现金类出资账户（同币种准入）。
fn setup_both_ends() -> (rusqlite::Connection, rusqlite::Connection) {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    for conn in [&conn_a, &conn_b] {
        seed_investment_setup(conn, "acc-inv", "inst-1");
        seed_account(conn, "acc-fund-a", "卡A", "bank", "USD", 0);
        seed_account(conn, "acc-fund-b", "卡B", "bank", "USD", 0);
        // 种子直插的账户无余额缓存行：夹具经既有接缝全量回填一次（ADR-0067）。
        crate::accounts::balance::refresh_all_account_balances(conn).unwrap();
    }
    (conn_a, conn_b)
}

/// 三端缓存余额两端相等的统一判据（重放端余额与本地一致，逐账户断言）。
fn assert_balances_converge(conn_a: &rusqlite::Connection, conn_b: &rusqlite::Connection) {
    for account in ["acc-inv", "acc-fund-a", "acc-fund-b"] {
        assert_eq!(
            cached_balance(conn_a, account).unwrap(),
            cached_balance(conn_b, account).unwrap(),
            "重放端 {account} 余额应与本地一致"
        );
    }
    assert_balance_cache_matches_realtime(conn_a);
    assert_balance_cache_matches_realtime(conn_b);
}

#[test]
fn funding_buy_sell_create_replay_converges_balances() {
    let (conn_a, conn_b) = setup_both_ends();

    // A 端：出资买入（buy 155 分记出资账户）→ 同步 → 卖出（sell 60 分记出资账户）→ 同步。
    let buy_id = protocol::create(&conn_a, buy_input("acc-inv", "inst-1", Some("acc-fund-a")))
        .unwrap()
        .id;
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    // 行业务字段逐列一致（含出资端）+ 三端余额一致：出资账户 −155、投资账户现金腿 0。
    let expected = read_transaction(&conn_a, &buy_id).unwrap();
    assert_eq!(expected.funding_account_id.as_deref(), Some("acc-fund-a"));
    assert_eq!(read_transaction(&conn_b, &buy_id).unwrap(), expected);
    assert_balances_converge(&conn_a, &conn_b);
    assert_eq!(cached_balance(&conn_b, "acc-fund-a").unwrap(), -155);
    assert_eq!(cached_balance(&conn_b, "acc-inv").unwrap(), 0);

    let sell_id = protocol::create(&conn_a, sell_input("acc-inv", "inst-1", Some("acc-fund-a")))
        .unwrap()
        .id;
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    let expected = read_transaction(&conn_a, &sell_id).unwrap();
    assert_eq!(read_transaction(&conn_b, &sell_id).unwrap(), expected);
    assert_balances_converge(&conn_a, &conn_b);
    assert_eq!(
        cached_balance(&conn_b, "acc-fund-a").unwrap(),
        -155 + 60,
        "sell 记出资账户 +"
    );

    // 全量重投递：幂等跳过，余额无第二次效果。
    let reports = apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Skipped));
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn funding_update_replay_recalc_three_ends() {
    let (conn_a, conn_b) = setup_both_ends();
    let id = protocol::create(&conn_a, buy_input("acc-inv", "inst-1", Some("acc-fund-a")))
        .unwrap()
        .id;
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    // 修改：出资账户 a → b（三端受影响账户重算：a 回补、b 扣减、投资账户不动）。
    protocol::update(
        &conn_a,
        &id,
        buy_input("acc-inv", "inst-1", Some("acc-fund-b")),
    )
    .unwrap();
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);
    assert_balances_converge(&conn_a, &conn_b);
    assert_eq!(cached_balance(&conn_b, "acc-fund-a").unwrap(), 0);
    assert_eq!(cached_balance(&conn_b, "acc-fund-b").unwrap(), -155);
    assert_eq!(cached_balance(&conn_b, "acc-inv").unwrap(), 0);

    // 再修改：去掉出资账户（回到「现金腿记投资账户」的既有语义）。
    protocol::update(&conn_a, &id, buy_input("acc-inv", "inst-1", None)).unwrap();
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);
    assert!(expected.funding_account_id.is_none());
    assert_balances_converge(&conn_a, &conn_b);
    assert_eq!(
        cached_balance(&conn_b, "acc-inv").unwrap(),
        -155,
        "无出资账户时现金腿记投资账户"
    );
    assert_eq!(cached_balance(&conn_b, "acc-fund-b").unwrap(), 0);
}

#[test]
fn funding_delete_replay_restores_funding_balance() {
    let (conn_a, conn_b) = setup_both_ends();
    let id = protocol::create(&conn_a, buy_input("acc-inv", "inst-1", Some("acc-fund-a")))
        .unwrap()
        .id;
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    assert_eq!(cached_balance(&conn_b, "acc-fund-a").unwrap(), -155);

    // 删除重放：出资账户余额回补，两端一致（与本地删除同一协议）。
    protocol::delete(&conn_a, &id).unwrap();
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);
    assert_eq!(expected.is_deleted, 1);
    assert_balances_converge(&conn_a, &conn_b);
    assert_eq!(cached_balance(&conn_b, "acc-fund-a").unwrap(), 0);
}

#[test]
fn legacy_op_without_funding_field_replays_with_old_semantics() {
    let (conn_a, conn_b) = setup_both_ends();
    let id = protocol::create(&conn_a, buy_input("acc-inv", "inst-1", Some("acc-fund-a")))
        .unwrap()
        .id;

    // 把 op 改写成旧格式（V023 前设备产出）：行载荷无出资字段、schema 版本 22。
    let mut legacy: serde_json::Value = serde_json::from_str(&wire_out(&conn_a)[0]).unwrap();
    legacy["schema_version"] = serde_json::json!(22);
    let removed = legacy["command"]["payload"]["row"]
        .as_object_mut()
        .unwrap()
        .remove("funding_account_id")
        .is_some();
    assert!(removed, "前置：op 行载荷应携带出资字段");
    let raw = serde_json::to_string(&legacy).unwrap();

    // 旧格式 op 重放不挂起、不报错：缺失的出资字段按缺省 None 执行。
    let reports = wire_in(&conn_b, &[raw]);
    assert!(
        matches!(reports[0].outcome, OpOutcome::Applied),
        "旧格式 op 应照常应用，实际: {:?}",
        reports[0].outcome
    );

    // 语义不变（ADR-0096 决策 8 的旧载荷方向）：出资视为未填，现金腿记投资账户。
    let row = read_transaction(&conn_b, &id).unwrap();
    assert_eq!(row.funding_account_id, None);
    assert_eq!(row.kind, TransactionKind::Buy);
    assert_eq!(row.amount_cents, 155);
    assert_eq!(cached_balance(&conn_b, "acc-inv").unwrap(), -155);
    assert_eq!(cached_balance(&conn_b, "acc-fund-a").unwrap(), 0);
    assert_balance_cache_matches_realtime(&conn_b);

    // 已落日志（幂等重投跳过），不残留挂起。
    let again = ingest_ops(&conn_b, &[serde_json::to_string(&legacy).unwrap()]).unwrap();
    assert_eq!(again[0].outcome, OpOutcome::Skipped);
    assert!(parked_ops(&conn_b).unwrap().is_empty());
}
