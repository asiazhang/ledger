//! 基金转换（convert）的语义命令重放（issue #980 / ADR-0099 决策 6）：
//! 载荷携 `ConvertCommandFields`（含源端算定的结转成本），重放端本地重建 FIFO
//! 快照并落转出消耗记录——create / update / delete 三 op 重放后两端批次、结转
//! 成本、持仓与盈亏完全一致（确定性重放），重复投递幂等；旧 schema 版本 op 挂起、
//! 升级后重投递自然重试成功；旧载荷（无 convert 成员）向后兼容，而旧端产出的
//! 转换 op（有行无字段）经 kind 防御臂挂起、不静默错账。
//!
//! 断言权威 = 同步引擎公开接口（`read_ops` / `ingest_ops` / `parked_ops`）与行为层
//! 公开入口；批次/消耗/匹配的判据读取以**锚定交易 id** 为自然键（批次行 id 是各端
//! 本地事实，不参与状态等值判定——与既有投资重放测试同一取舍）。

use rusqlite::Connection;

use super::super::{OpOutcome, parked_ops, read_ops};
use super::common::{read_lot, read_lot_sale, read_transaction, seed_device, wire_in, wire_out};
use crate::accounts::balance::cached_balance;
use crate::db;
use crate::test_support::{
    self, assert_balance_cache_matches_realtime, seed_account, seed_instrument,
};
use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::write::protocol;

/// 双端铺垫：投资账户 + 转出/转入两标的（币种同账户，1:1 折算）。
fn seed_both_ends() -> (Connection, Connection) {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-cv", "基金户", "investment", "CNY", 0);
        seed_instrument(conn, "inst-out", "006793", "转出基金", "CNY", "unknown");
        seed_instrument(conn, "inst-in", "519700", "转入基金", "CNY", "unknown");
        // 种子直插的账户无写路径钩子：夹具经既有接缝全量回填一次，缓存行才可比对。
        crate::accounts::balance::refresh_all_account_balances(conn).unwrap();
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

/// 转换输入（金额为确认单权威；行金额锚点由后端按 FIFO 结转成本重算）。
/// 参数众多是输入构造器的固有形状（同 `investment::tests::common` 同款取舍）。
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

/// 转换转出消耗快照：按消耗批次锚定交易 id 排序的 (锚定交易, 数量, 每份成本, 结转成本)。
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

/// 转换明细行快照：(转出份额, 反算单价, 手续费, 转入标的, 转入份额, 转出金额, 转入金额)。
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

/// 全量重投递全部 op：逐条幂等跳过（无第二次效果）。
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
    // A 端：两笔建仓（单价 1.00 / 2.00 元）→ 转换 15 份（耗尽首批 10 + 部分耗二批 5）→
    // 卖出转入份额 5 份（成本随转换结转流入已实现盈亏）。
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

    // 转换行：行金额锚点 = 结转成本 2000 分（首批耗尽 1000 + 二批部分 1000），
    // 明细行单价 = 2250 × 100 ÷ 15 = 15000（1.50 元）。
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

    // 转出腿逐批次消耗：首批耗尽按「批次数额 − 此前已消耗」闭合、二批按数量比例单次舍入。
    let expected_conversions = vec![
        (buy1.clone(), 10.0, 10_000, 1_000),
        (buy2.clone(), 5.0, 20_000, 1_000),
    ];
    assert_eq!(read_conversions(&conn_a, &convert_id), expected_conversions);
    assert_eq!(
        read_conversions(&conn_b, &convert_id),
        expected_conversions,
        "本地重建 FIFO 快照 ⇒ 同一逐批次消耗与结转成本"
    );

    // 转出批次与转入批次（每份成本 = 结转成本 ÷ 转入份额单次舍入 = 13333）。
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

    // 卖出转入份额 5 份：成本 667（round(5 × 13333 ÷ 100)）、盈亏 1500 − 667 = 833。
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

    // 余额（含六度量系数全 0 的转换：转换前后不变）两端一致；缓存 == 实时。
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

    // 就地修改：10 份 → 5 份（先回补旧转出腿，再按新输入重消耗）。
    protocol::update(
        &conn_a,
        &convert_id,
        convert_input("acc-cv", "inst-out", "inst-in", 5.0, 5.0, 550, 550, 0),
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));

    // 两端同态：转出批次回补后只消耗 5、转入批次重建为 5 份、消耗记录唯一一条。
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

    // 删除重放：转出腿逐批次精确回补、转入批次与转换明细行整批清理、转换行软删。
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

    // 依赖倒挂：只投递转换 op——本地 FIFO 快照无可消耗批次，可卖数量守卫挂起、不落日志。
    let reports = wire_in(&conn_b, &convert_wire);
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { code, .. } if code == "trade.insufficient-holding"),
        "可卖数量不足挂起并携带既有码：{:?}",
        reports[0]
    );
    assert!(read_ops(&conn_b).unwrap().is_empty(), "挂起不落日志");
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);

    // 买入 op 补达后重投递转换 op：本地重建 FIFO 快照成功，重放收敛并出队。
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
    // 依赖先到位，使挂起原因只可能是 schema 版本偏斜。
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

    // 升级后（本端版本追平）重投递同一 op：自然重试成功并出队，两端收敛。
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

    // 旧格式 op（#980 前设备产出）：载荷无 convert 成员；可选成员缺省即 None，照常应用。
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

    // 已落日志（幂等重投跳过），不残留挂起。
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

    // 旧端产出的转换 op（有行无 convert 字段）：kind 防御臂显式失败挂起，
    // 不落半套持仓副作用、不静默错账（ADR-0099 决策 6 的双保险之一）。
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

    // 源端结转成本与本地 FIFO 重建不一致（载荷被篡改/本地快照发散）：显式失败
    // 挂起，不静默落出错误成本基础（ADR-0099 决策 6）。
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

    // 伪造载荷带转入账户（本地录入必被 `trade.convert-to-account-forbidden` 拒绝）：
    // 重放经同一接缝守卫，不绕开本地不变量。账户存活校验先过（指向存活的投资账户）。
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
