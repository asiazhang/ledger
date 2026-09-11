//! 份额调整（split）的语义命令重放（issue #1053 / ADR-0106 决策 9）：载荷只携 Δ
//! 与源端**最终持仓 / 批次总成本**两个比对锚点，重放端以本地 FIFO 批次快照独立
//! 重建重述并比对；create / update / delete 三 op 重放后两端批次、审计、持仓与成本
//! 完全一致（确定性重放），重复投递幂等；依赖倒挂（买入 op 未达）以零持仓守卫挂起、
//! 补齐后重投递自然重试；快照发散（载荷篡改）以 `transaction.split-restatement-mismatch`
//! 码化挂起、不静默错账；旧端产出的 split op（有行无字段）经 kind 防御臂挂起。
//!
//! 断言权威 = 同步引擎公开接口（`read_ops` / `ingest_ops` / `parked_ops`）与行为层
//! 公开入口；批次/审计的判据读取以**锚定交易 id** 为自然键（批次行 id 是各端本地事实，
//! 不参与状态等值判定——与既有投资重放测试同一取舍）。

use rusqlite::Connection;

use super::super::{OpOutcome, parked_ops, read_ops};
use super::common::{read_lot, read_transaction, seed_device, wire_in, wire_out};
use crate::accounts::balance::cached_balance;
use crate::test_support::{
    self, assert_balance_cache_matches_realtime, seed_account, seed_instrument,
};
use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::behavior;

/// 双端铺垫：投资账户 + 标的（币种同账户）；种子直插的账户无写路径钩子，夹具经既有
/// 接缝全量回填一次，缓存行才可比对。
fn seed_both_ends() -> (Connection, Connection) {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-sp", "股票户", "investment", "CNY", 0);
        seed_instrument(conn, "inst-sp", "502010", "证券基金", "CNY", "unknown");
        crate::accounts::balance::refresh_all_account_balances(conn).unwrap();
    }
    (conn_a, conn_b)
}

fn buy_input(
    account_id: &str,
    instrument_id: &str,
    quantity: f64,
    price_cents: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(quantity),
        price_cents: Some(price_cents),
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
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
        date: "2026-01-20".into(),
        ..buy_input(account_id, instrument_id, quantity, price_cents)
    }
}

fn split_input(account_id: &str, instrument_id: &str, delta: f64) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Split,
        quantity: Some(delta),
        price_cents: None,
        date: "2026-02-01".into(),
        ..buy_input(account_id, instrument_id, 0.0, 0)
    }
}

/// 某标的的在用批次快照（按 rowid 序）：(锚定交易 id, 初始数量, 剩余数量, 每份成本)。
fn lots_of(conn: &Connection, instrument_id: &str) -> Vec<(String, f64, f64, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT buy_transaction_id, initial_quantity, remaining_quantity, cost_per_unit_cents \
             FROM security_lots WHERE instrument_id=?1 AND remaining_quantity > 0 ORDER BY rowid",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![instrument_id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

/// 该 split 的逐批次重述审计（按批次 rowid 序）：(重述前剩余, 重述后剩余, 重述后每份成本)。
fn adjustments_of(conn: &Connection, split_id: &str) -> Vec<(f64, f64, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT a.remaining_quantity_before, a.remaining_quantity_after, a.cost_per_unit_cents_after \
             FROM security_lot_adjustments a JOIN security_lots l ON l.id = a.lot_id \
             WHERE a.transaction_id=?1 ORDER BY l.rowid",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![split_id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

/// split 扩展行快照：(带符号 Δ, 行单价)。无扩展行返回 None。
fn split_extension(conn: &Connection, split_id: &str) -> Option<(f64, Option<i64>)> {
    let sql = "SELECT quantity, price_cents FROM security_transactions \
               WHERE transaction_id=?1 AND action='split'";
    conn.query_row(sql, [split_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok()
}

/// 全量重投递全部 op：逐条幂等跳过（无第二次效果）。
fn assert_full_redelivery_is_idempotent(conn_a: &Connection, conn_b: &Connection) {
    let reports = wire_in(conn_b, &wire_out(conn_a));
    assert!(
        reports.iter().all(|r| r.outcome == OpOutcome::Skipped),
        "重投递应全部幂等跳过：{reports:?}"
    );
    assert_eq!(read_ops(conn_a).unwrap(), read_ops(conn_b).unwrap());
}

/// A 端铺垫两批次 + 部分卖出：100 份 @1.00 元、200 份 @2.00 元、卖 40 份 @1.00 元。
/// 返回 (买1 id, 买2 id, 卖 id)。
fn seed_two_lots_with_partial_sell(conn: &Connection) -> (String, String, String) {
    let buy1 = behavior::create(conn, buy_input("acc-sp", "inst-sp", 100.0, 10_000))
        .unwrap()
        .id;
    let buy2 = behavior::create(conn, buy_input("acc-sp", "inst-sp", 200.0, 20_000))
        .unwrap()
        .id;
    let sell = behavior::create(conn, sell_input("acc-sp", "inst-sp", 40.0, 10_000))
        .unwrap()
        .id;
    (buy1, buy2, sell)
}

/// 闭环判据（两端同态）：批次、重述审计、split 扩展行、余额缓存逐项比对。
fn assert_split_converged(conn_a: &Connection, conn_b: &Connection, split_id: &str) {
    assert_eq!(
        lots_of(conn_b, "inst-sp"),
        lots_of(conn_a, "inst-sp"),
        "批次（数量与每份成本）重放后一致"
    );
    assert_eq!(
        adjustments_of(conn_b, split_id),
        adjustments_of(conn_a, split_id),
        "逐批次重述审计重放后一致"
    );
    assert_eq!(
        split_extension(conn_b, split_id),
        split_extension(conn_a, split_id),
        "split 扩展行（Δ 与单价）重放后一致"
    );
    assert_eq!(
        cached_balance(conn_b, "acc-sp").unwrap(),
        cached_balance(conn_a, "acc-sp").unwrap()
    );
    assert_balance_cache_matches_realtime(conn_b);
}

#[test]
fn split_create_replay_converges_lots_and_restatement() {
    let (conn_a, conn_b) = seed_both_ends();
    let (buy1, buy2, _sell) = seed_two_lots_with_partial_sell(&conn_a);
    // 重述前权威总成本：批次 1 锚点 10000 − 记录消耗 4000 = 6000；批次 2 = 40000。
    let split_id = behavior::create(&conn_a, split_input("acc-sp", "inst-sp", 30.0))
        .unwrap()
        .id;

    // 载荷只携 Δ 与两个比对锚点（ADR-0106 决策 9）：无逐批次重述结果。
    let wire = wire_out(&conn_a);
    let split_op: serde_json::Value = serde_json::from_str(wire.last().unwrap()).unwrap();
    let fields = &split_op["command"]["payload"]["split"];
    assert_eq!(fields["delta_quantity"], serde_json::json!(30.0));
    assert_eq!(fields["final_quantity"], serde_json::json!(290.0));
    assert_eq!(fields["total_cost_cents"], serde_json::json!(46_000));
    assert!(
        split_op["command"]["payload"]["investment"].is_null(),
        "份额调整不携 buy/sell 语义字段"
    );
    assert_eq!(
        split_op["command"]["payload"]["row"]["amount_cents"],
        serde_json::json!(0),
        "无现金腿：行金额恒 0"
    );

    wire_in(&conn_b, &wire);
    assert_split_converged(&conn_a, &conn_b, &split_id);

    // 尾差归末批次（确定性重放的批次序依据）：持仓 260 → 290，f = 290/260。
    // 批次 1（非末）独立取整：cpu = round(10000 ÷ f) = 8966；批次 2（末）以权威
    // 总成本 46000 分倒推闭合：cpu = 17931。两端逐批次取值同一，不靠纪律。
    let lots_a = lots_of(&conn_a, "inst-sp");
    assert_eq!(lots_a.len(), 2);
    assert_eq!(lots_a[0].0, buy1);
    assert_eq!(lots_a[1].0, buy2);
    assert!((lots_a[0].2 - 60.0 * 290.0 / 260.0).abs() < 1e-9);
    assert_eq!(lots_a[0].3, 8_966, "非末批次独立取整稀释 round(10000 ÷ f)");
    assert!((lots_a[1].2 - 200.0 * 290.0 / 260.0).abs() < 1e-9);
    assert_eq!(lots_a[1].3, 17_931, "末批次倒推闭合吸收全部舍入尾差");
    let holding: f64 = lots_a.iter().map(|l| l.2).sum();
    assert!((holding - 290.0).abs() < 1e-9, "最终持仓 = 260 + Δ30 = 290");

    // split 零已实现盈亏、行金额恒 0（无现金腿）。
    assert_eq!(split_extension(&conn_a, &split_id), Some((30.0, None)));
    assert_eq!(
        read_transaction(&conn_a, &split_id).unwrap().amount_cents,
        0
    );
    assert_eq!(
        read_transaction(&conn_b, &split_id).unwrap(),
        read_transaction(&conn_a, &split_id).unwrap()
    );

    assert_full_redelivery_is_idempotent(&conn_a, &conn_b);
}

#[test]
fn split_dependency_missing_parks_then_redelivery_self_heals() {
    let (conn_a, conn_b) = seed_both_ends();
    let buy1 = behavior::create(&conn_a, buy_input("acc-sp", "inst-sp", 100.0, 10_000))
        .unwrap()
        .id;
    let split_id = behavior::create(&conn_a, split_input("acc-sp", "inst-sp", 30.0))
        .unwrap()
        .id;
    let wire = wire_out(&conn_a);
    let (buy_wire, split_wire) = (vec![wire[0].clone()], vec![wire[1].clone()]);

    // 依赖倒挂：只投递 split op——本地零在用持仓，无从重述，零持仓守卫挂起、不落日志。
    let reports = wire_in(&conn_b, &split_wire);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "trade.split-no-holding"),
        "零持仓（前序买入 op 未达）挂起并携带既有码：{:?}",
        reports[0]
    );
    assert!(read_ops(&conn_b).unwrap().is_empty(), "挂起不落日志");
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);
    assert!(
        read_transaction(&conn_b, &split_id).is_none(),
        "不落 split 行"
    );
    assert!(split_extension(&conn_b, &split_id).is_none());

    // 买入 op 补达后重投递 split op：本地重建重述成功，重放收敛并出队。
    wire_in(&conn_b, &buy_wire);
    assert_eq!(
        parked_ops(&conn_b).unwrap().len(),
        1,
        "buy 补达不自动解决挂起"
    );
    let reports = wire_in(&conn_b, &split_wire);
    assert_eq!(reports[0].outcome, OpOutcome::Applied);
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert_eq!(read_lot(&conn_b, &buy1), read_lot(&conn_a, &buy1));
    assert_split_converged(&conn_a, &conn_b, &split_id);
}

#[test]
fn split_op_with_divergent_restatement_parks_without_booking() {
    let (conn_a, conn_b) = seed_both_ends();
    behavior::create(&conn_a, buy_input("acc-sp", "inst-sp", 100.0, 10_000)).unwrap();
    let split_id = behavior::create(&conn_a, split_input("acc-sp", "inst-sp", 30.0))
        .unwrap()
        .id;
    let wire = wire_out(&conn_a);
    wire_in(&conn_b, &[wire[0].clone()]);

    // 源端锚点被篡改（或本地快照发散）：本地重建的最终持仓与源端携带值不一致，
    // 显式失败挂起，不静默落出错误持仓与成本基础（ADR-0106 决策 9）。
    let mut tampered: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    tampered["command"]["payload"]["split"]["final_quantity"] = serde_json::json!(999.0);
    let reports = wire_in(&conn_b, &[serde_json::to_string(&tampered).unwrap()]);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "transaction.split-restatement-mismatch"),
        "最终持仓锚点发散应码化挂起，实际: {:?}",
        reports[0]
    );
    assert!(
        read_transaction(&conn_b, &split_id).is_none(),
        "不落 split 行"
    );
    assert!(
        adjustments_of(&conn_b, &split_id).is_empty(),
        "不落重述审计"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);

    // 总成本锚点发散同规挂起。
    let mut tampered: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    tampered["command"]["payload"]["split"]["total_cost_cents"] = serde_json::json!(1);
    let reports = wire_in(&conn_b, &[serde_json::to_string(&tampered).unwrap()]);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "transaction.split-restatement-mismatch"),
        "总成本锚点发散应码化挂起，实际: {:?}",
        reports[0]
    );
}

#[test]
fn legacy_split_op_without_fields_parks_without_booking() {
    let (conn_a, conn_b) = seed_both_ends();
    behavior::create(&conn_a, buy_input("acc-sp", "inst-sp", 100.0, 10_000)).unwrap();
    let split_id = behavior::create(&conn_a, split_input("acc-sp", "inst-sp", 30.0))
        .unwrap()
        .id;
    let wire = wire_out(&conn_a);
    wire_in(&conn_b, &[wire[0].clone()]);

    // 旧端产出的 split op（有行无份额调整字段）：kind 防御臂显式失败挂起，不落半套
    // 副作用、不静默落出未经校验的重述（与 convert 旧载荷同规，ADR-0099 决策 6）。
    let mut legacy: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    let removed = legacy["command"]["payload"]
        .as_object_mut()
        .unwrap()
        .remove("split")
        .is_some();
    assert!(removed, "前置：op 载荷应携带 split 成员");
    let reports = wire_in(&conn_b, &[serde_json::to_string(&legacy).unwrap()]);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "transaction.split-fields-missing"),
        "缺份额调整字段的 split op 应码化挂起，实际: {:?}",
        reports[0]
    );
    assert!(
        read_transaction(&conn_b, &split_id).is_none(),
        "不落 split 行"
    );
    assert!(
        adjustments_of(&conn_b, &split_id).is_empty(),
        "不落重述审计"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);
}

#[test]
fn split_update_and_delete_replay_converge() {
    let (conn_a, conn_b) = seed_both_ends();
    let (buy1, buy2, _sell) = seed_two_lots_with_partial_sell(&conn_a);
    let split_id = behavior::create(&conn_a, split_input("acc-sp", "inst-sp", 20.0))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_split_converged(&conn_a, &conn_b, &split_id);

    // 就地修改：Δ 20 → 50（先按审计精确回补旧重述，再按新 Δ 重建重述）。
    behavior::update(&conn_a, &split_id, split_input("acc-sp", "inst-sp", 50.0)).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(
        split_extension(&conn_a, &split_id),
        Some((50.0, None)),
        "修改后 Δ 更新"
    );
    assert_split_converged(&conn_a, &conn_b, &split_id);

    // 删除重放：按重述审计逐批次精确回补（数量与每份成本含舍入一并还原），扩展行清除。
    behavior::delete(&conn_a, &split_id).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(read_transaction(&conn_a, &split_id).unwrap().is_deleted, 1);
    assert_eq!(read_transaction(&conn_b, &split_id).unwrap().is_deleted, 1);
    for conn in [&conn_a, &conn_b] {
        assert_eq!(
            lots_of(conn, "inst-sp"),
            vec![
                (buy1.clone(), 100.0, 60.0, 10_000),
                (buy2.clone(), 200.0, 200.0, 20_000),
            ],
            "删除重放后批次精确回补到重述前状态"
        );
        assert!(split_extension(conn, &split_id).is_none(), "扩展行清除");
        assert!(adjustments_of(conn, &split_id).is_empty(), "审计清除");
    }
    assert_balance_cache_matches_realtime(&conn_b);

    assert_full_redelivery_is_idempotent(&conn_a, &conn_b);
}
