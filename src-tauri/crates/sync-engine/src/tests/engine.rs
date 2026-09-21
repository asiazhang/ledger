//! 同步引擎闭环（issue #855 / ADR-0091）：A 端写 → B 端重放后账本状态一致；
//! 同一 op 重复投递不产生第二次效果（按 op 标识幂等）；双端 op 收敛到同一全序。
//!
//! 双端场景 = 同进程两个引擎实例（各自内存库），Transport 引入前的直接 op
//! 传递（#859 接线）；重放复用既有行为编排入口，不新增写接缝。
//! 参考数据（账户字典）的同步归 #860，本目录两端以同一夹具等量种子。

use super::super::{ApplyReport, DomainCommand, OpOutcome, apply_ops, parked_ops, read_ops};
use super::common::{make_expense, read_transaction, wire_in, wire_out};
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support::{
    self, assert_balance_cache_matches_realtime, seed_account, seed_fx_rate_history,
};

/// A 端记账 → B 端重放：业务字段逐列一致 + 派生缓存经既有接缝重算自洽。
#[test]
fn a_writes_b_replays_ledger_converges() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let created = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();

    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 1);

    let reports = apply_ops(&conn_b, &ops).unwrap();
    assert_eq!(
        reports,
        vec![ApplyReport {
            op_id: ops[0].op_id.clone(),
            outcome: OpOutcome::Applied,
        }]
    );

    // 账本状态一致：业务字段逐列相等（审计列是各端本地事实，不参与判定）。
    let expected = read_transaction(&conn_a, &created.id).unwrap();
    assert_eq!(read_transaction(&conn_b, &created.id).unwrap(), expected);
    assert_eq!(expected.amount_native_cents, 10000);
    assert_eq!(expected.is_deleted, 0);
    // 派生数据不进日志：B 端经既有接缝重算且自洽（ADR-0067 延伸）。
    assert_balance_cache_matches_realtime(&conn_b);
}

/// 折算来源留痕随 op 收敛（#1548 / ADR-0011 修订）：A 端外币行折算后，汇率值与
/// 来源随行载荷搬运，B 端**无本地汇率历史**也收敛到与 A 端一致的行（源端折算
/// 语义的留痕面：重放不重折算，也不依赖重放端的汇率数据）。
#[test]
fn fx_rate_trace_rides_op_and_converges() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-hkd", "港币户", "cash", "HKD", 0);
    seed_account(&conn_b, "acc-hkd", "港币户", "cash", "HKD", 0);
    // 只种 A 端汇率历史：B 端序列为空，重放仍必须得到同一行（含留痕）。
    seed_fx_rate_history(&conn_a, "fxh-sync", "HKD", "CNY", "2026-01-05", 0.9);

    let mut input = make_expense("acc-hkd", 10000, "港币午饭");
    input.currency_code = "HKD".into();
    input.date = "2026-01-07".into();
    let created = protocol::create(&conn_a, input).unwrap();

    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    let expected = read_transaction(&conn_a, &created.id).unwrap();
    assert_eq!(expected.amount_native_cents, 9000);
    assert_eq!(expected.fx_rate_used, Some(0.9));
    assert_eq!(
        read_transaction(&conn_b, &created.id).unwrap(),
        expected,
        "重放端应收敛到与源端一致的行（含折算留痕）"
    );
    assert_balance_cache_matches_realtime(&conn_b);
}

/// 旧格式 op（行载荷无留痕字段，V029 前设备产出）前向兼容：重放不报错、
/// 按缺省 `None` 落库——与「写入时点早于留痕功能」的存量行 NULL 语义一致。
#[test]
fn legacy_op_without_fx_trace_fields_replays_with_null_trace() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-hkd", "港币户", "cash", "HKD", 0);
    seed_account(&conn_b, "acc-hkd", "港币户", "cash", "HKD", 0);
    seed_fx_rate_history(&conn_a, "fxh-sync", "HKD", "CNY", "2026-01-05", 0.9);

    let mut input = make_expense("acc-hkd", 10000, "港币午饭");
    input.currency_code = "HKD".into();
    input.date = "2026-01-07".into();
    let created = protocol::create(&conn_a, input).unwrap();

    // 把 op 改写成旧格式（V029 前设备产出）：行载荷无两个留痕字段、schema 版本 22。
    let mut legacy: serde_json::Value = serde_json::from_str(&wire_out(&conn_a)[0]).unwrap();
    legacy["schema_version"] = serde_json::json!(22);
    let row = legacy["command"]["payload"]["row"].as_object_mut().unwrap();
    let removed = row.remove("fx_rate_used").is_some() && row.remove("fx_rate_source").is_some();
    assert!(removed, "前置：op 行载荷应携带留痕字段");
    let raw = serde_json::to_string(&legacy).unwrap();

    let reports = wire_in(&conn_b, &[raw]);
    assert!(
        matches!(reports[0].outcome, OpOutcome::Applied),
        "旧格式 op 应照常应用，实际: {:?}",
        reports[0].outcome
    );
    let row = read_transaction(&conn_b, &created.id).unwrap();
    assert_eq!(row.amount_native_cents, 9000, "源端折算结果照常搬运");
    assert_eq!(row.fx_rate_used, None, "旧载荷缺省：无汇率留痕");
    assert_eq!(row.fx_rate_source, None, "旧载荷缺省：无来源留痕");
    assert!(parked_ops(&conn_b).unwrap().is_empty());
}

#[test]
fn redelivery_of_same_ops_is_idempotent() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    let ops = read_ops(&conn_a).unwrap();
    apply_ops(&conn_b, &ops).unwrap();

    // 整批重投：全部按 op 标识跳过，账本无第二次效果。
    let reports = apply_ops(&conn_b, &ops).unwrap();
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Skipped));

    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "重复投递不产生第二笔交易");

    // 部分重复（旧 op + 新 op 混合投递）同样安全。
    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();
    let all_a = read_ops(&conn_a).unwrap();
    assert_eq!(all_a.len(), 2);
    let mut mixed = ops;
    mixed.push(all_a[1].clone());
    let reports = apply_ops(&conn_b, &mixed).unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, OpOutcome::Skipped, "旧 op 已知，跳过");
    assert_eq!(reports[1].outcome, OpOutcome::Applied, "新 op 执行");
}

#[test]
fn update_and_delete_replay_keep_both_sides_equal() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let id = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id;
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    // 修改（全字段替换语义，含折算结果更新）。
    protocol::update(&conn_a, &id, make_expense("acc-1", 25000, "午饭（改）")).unwrap();
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);
    assert_eq!(expected.amount_native_cents, 25000);

    // 删除（软删）。
    protocol::delete(&conn_a, &id).unwrap();
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);
    assert_eq!(expected.is_deleted, 1);
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn cross_device_ops_converge_to_same_total_order() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    // 两端并发各记一笔（端内时钟同为 1，全序由 DeviceId tiebreak）。
    protocol::create(&conn_a, make_expense("acc-1", 10000, "A 记")).unwrap();
    protocol::create(&conn_b, make_expense("acc-1", 20000, "B 记")).unwrap();

    // 互换日志（各自重放对方，自己的 op 按标识跳过）。
    let ops_a = read_ops(&conn_a).unwrap();
    let ops_b = read_ops(&conn_b).unwrap();
    apply_ops(&conn_b, &ops_a).unwrap();
    apply_ops(&conn_a, &ops_b).unwrap();

    // 两端对同一批 op 排出唯一一致的全序，日志与账本双双一致。
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
    let both = read_ops(&conn_a).unwrap();
    assert_eq!(both.len(), 2);
    assert_eq!(both[0].clock, both[1].clock, "两端并发：端内时钟同为 1");
    assert_ne!(both[0].device_id, both[1].device_id);
    assert!(
        both[0].device_id < both[1].device_id,
        "同钟并列按 DeviceId 字典序 tiebreak"
    );
    for op in &both {
        let DomainCommand::Transaction(ledger_transaction::TransactionCommand::Create {
            id, ..
        }) = &op.command
        else {
            panic!("应为交易 create 命令");
        };
        assert_eq!(
            read_transaction(&conn_a, id),
            read_transaction(&conn_b, id),
            "两端对同一笔交易的账本状态一致"
        );
    }
    assert_balance_cache_matches_realtime(&conn_a);
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn apply_ops_on_empty_batch_is_noop() {
    let conn = test_support::open();
    let reports = apply_ops(&conn, &[]).unwrap();
    assert!(reports.is_empty());
    assert!(read_ops(&conn).unwrap().is_empty());
}

#[test]
fn dependency_failure_parks_without_blocking_batch_and_redelivery_applies() {
    // 挂起队列（issue #856 / ADR-0091 决策 6）：外键依赖失败（参考数据未同步）
    // 的 op 进挂起队列并发码化错误，不阻塞其余 op 重放，不静默丢弃；依赖方
    // 补齐后重投递自然重试，成功即出队。
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    // op1：合法（两端账户齐备）；op2：引用仅 A 端有的账户（B 端外键依赖失败）。
    seed_account(&conn_a, "acc-a-only", "A 独有账户", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 10000, "好的")).unwrap();
    let bad = protocol::create(&conn_a, make_expense("acc-a-only", 500, "坏引用"))
        .unwrap()
        .id;
    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 2);

    let reports = apply_ops(&conn_b, &ops).unwrap();
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Applied),
        "好 op 不受阻塞"
    );
    assert!(
        matches!(&reports[1].outcome, OpOutcome::Parked { code, .. } if !code.is_empty()),
        "坏引用挂起并发码化错误"
    );
    // 挂起 op 不落日志（未应用），账本无其效果，也不自动补建缺失账户。
    assert!(
        !read_ops(&conn_b)
            .unwrap()
            .iter()
            .any(|op| op.op_id == ops[1].op_id)
    );
    assert!(read_transaction(&conn_b, &bad).is_none());
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].op_id, ops[1].op_id);
    assert_eq!(parked[0].entity, "transaction");

    // 依赖方补齐（A 独有账户同步到达）：重投递同一 op 自然重试，成功即出队。
    seed_account(&conn_b, "acc-a-only", "A 独有账户", "cash", "CNY", 0);
    let reports = apply_ops(&conn_b, &ops).unwrap();
    assert_eq!(reports[0].outcome, OpOutcome::Skipped, "已应用 op 幂等跳过");
    assert_eq!(reports[1].outcome, OpOutcome::Applied, "重投递自然重试成功");
    assert!(
        read_transaction(&conn_b, &bad).is_some(),
        "补齐后重投递落地"
    );
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert_balance_cache_matches_realtime(&conn_b);
}
