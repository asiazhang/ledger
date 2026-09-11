//! 现金分红（dividend）的语义命令重放（issue #1078 / ADR-0109）：载荷只增
//! 轻量可选成员——标的 id 复用投资字段（`quantity` / `price_cents` / `fee_cents`
//! 为占位，重放端只读 `instrument_id`），现金腿随归一化行携带；create / update /
//! delete 三 op 重放后两端交易行、`security_transactions` 扩展行、余额缓存与
//! 累计收益第三腿完全一致（确定性重放），重复投递幂等；标的不在时（前序标的
//! op 未达）以码化错误挂起、补齐后重投递自然重试；币种与到账账户分叉（载荷
//! 篡改 / 账户币种漂移）码化挂起，不静默落出错币记录。
//!
//! 断言权威 = 同步引擎公开接口（`read_ops` / `ingest_ops` / `parked_ops`）与行为层
//! 公开入口；扩展行以交易 id 为自然键读取（无逐端本地 id 参与状态等值判定）。

use rusqlite::Connection;

use super::super::{OpOutcome, parked_ops, read_ops};
use super::common::{read_transaction, seed_device, wire_in, wire_out};
use crate::accounts::balance::cached_balance;
use crate::investment::query_cumulative_pnl_summary;
use crate::test_support::{
    self, assert_balance_cache_matches_realtime, seed_account, seed_instrument,
};
use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::behavior;

/// 双端铺垫：投资账户 + 标的（币种同账户）。种子直插的账户无写路径钩子，
/// 夹具经既有接缝全量回填一次，缓存行才可比对。
fn seed_both_ends() -> (Connection, Connection) {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-dv", "股票户", "investment", "CNY", 0);
        seed_instrument(conn, "inst-dv", "502010", "证券基金", "CNY", "unknown");
        crate::accounts::balance::refresh_all_account_balances(conn).unwrap();
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
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Dividend,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
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
    }
}

/// dividend 扩展行快照：(标的 id, 份额, 单价)。无扩展行返回 None。
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

/// 累计收益第三腿小计（指定币种，缺组即 0）。
fn cumulative_of(conn: &Connection, currency: &str) -> i64 {
    query_cumulative_pnl_summary(conn)
        .unwrap()
        .iter()
        .find(|g| g.currency_code == currency)
        .map(|g| g.cumulative_pnl_cents)
        .unwrap_or(0)
}

/// 闭环判据（两端同态）：交易行、扩展行、余额缓存、累计收益第三腿逐项比对。
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
    let tx_id = behavior::create(&conn_a, dividend_input("acc-dv", "inst-dv", 3000, "现场A"))
        .unwrap()
        .id;

    // 载荷：标的 id 复用投资字段（占位零值），现金腿随归一化行。
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

    // update op（全字段替换）：摘除并重建扩展行、余额随之更新。
    behavior::update(
        &conn_a,
        &tx_id,
        dividend_input("acc-dv", "inst-dv", 5000, "现场B"),
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_dividend_converged(&conn_a, &conn_b, &tx_id);
    assert_eq!(cached_balance(&conn_a, "acc-dv").unwrap(), 5000);
    assert_eq!(cumulative_of(&conn_a, "CNY"), 5000);

    // delete op：扩展行摘除、交易行软删、余额回退。
    behavior::delete(&conn_a, &tx_id).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_dividend_converged(&conn_a, &conn_b, &tx_id);
    assert_eq!(read_transaction(&conn_a, &tx_id).unwrap().is_deleted, 1);
    assert_eq!(read_transaction(&conn_b, &tx_id).unwrap().is_deleted, 1);
    assert_eq!(dividend_extension(&conn_a, &tx_id), None);
    assert_eq!(dividend_extension(&conn_b, &tx_id), None);
    assert_eq!(cached_balance(&conn_a, "acc-dv").unwrap(), 0);
    assert_eq!(cached_balance(&conn_b, "acc-dv").unwrap(), 0);

    // 全量重投递：逐条幂等跳过（无第二次效果），两端日志一致。
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
    // B 端尚无该标的（真实世界对应标的 op 未达）：清空 B 的标的行模拟倒挂。
    conn_b
        .execute("DELETE FROM instruments WHERE id='inst-dv'", [])
        .unwrap();
    let tx_id = behavior::create(&conn_a, dividend_input("acc-dv", "inst-dv", 3000, "现场A"))
        .unwrap()
        .id;
    let wire = wire_out(&conn_a);

    // 标的不在：码化挂起、不落日志、不落行。
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

    // 依赖补齐（标的行到达 B 端）后重投递：成功、出队、两端同态。
    seed_instrument(&conn_b, "inst-dv", "502010", "证券基金", "CNY", "unknown");
    let reports = wire_in(&conn_b, &wire);
    assert_eq!(reports[0].outcome, OpOutcome::Applied);
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert_dividend_converged(&conn_a, &conn_b, &tx_id);
}

#[test]
fn dividend_currency_divergence_parks_without_booking() {
    let (conn_a, conn_b) = seed_both_ends();
    let tx_id = behavior::create(&conn_a, dividend_input("acc-dv", "inst-dv", 3000, "现场A"))
        .unwrap()
        .id;
    let wire = wire_out(&conn_a);

    // 载荷币种与目标端到账账户币种不一致（载荷篡改 / 账户币种漂移）：
    // 码化挂起，不静默落出错币记录（ADR-0109 到账账户币种一致性守卫）。
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
