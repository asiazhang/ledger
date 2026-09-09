//! 同步引擎闭环（issue #855 / ADR-0091）：A 端写 → B 端重放后账本状态一致；
//! 同一 op 重复投递不产生第二次效果（按 op 标识幂等）；双端 op 收敛到同一全序。
//!
//! 双端场景 = 同进程两个引擎实例（各自内存库），Transport 引入前的直接 op
//! 传递（#859 接线）；重放复用既有行为编排入口，不新增写接缝。
//! 参考数据（账户字典）的同步归 #860，本目录两端以同一夹具等量种子。

use super::super::{ApplyReport, DomainCommand, OpOutcome, apply_ops, read_ops};
use super::common::{make_expense, read_transaction};
use crate::test_support::{self, assert_balance_cache_matches_realtime, seed_account};
use crate::transaction::behavior;

/// A 端记账 → B 端重放：业务字段逐列一致 + 派生缓存经既有接缝重算自洽。
#[test]
fn a_writes_b_replays_ledger_converges() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let created = behavior::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();

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

#[test]
fn redelivery_of_same_ops_is_idempotent() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    behavior::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
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
    behavior::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();
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

    let id = behavior::create(&conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id;
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    // 修改（全字段替换语义，含折算结果更新）。
    behavior::update(&conn_a, &id, make_expense("acc-1", 25000, "午饭（改）")).unwrap();
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);
    assert_eq!(expected.amount_native_cents, 25000);

    // 删除（软删）。
    behavior::delete(&conn_a, &id).unwrap();
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
    behavior::create(&conn_a, make_expense("acc-1", 10000, "A 记")).unwrap();
    behavior::create(&conn_b, make_expense("acc-1", 20000, "B 记")).unwrap();

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
        let DomainCommand::Transaction(crate::transaction::TransactionCommand::Create {
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
fn failing_op_is_not_recorded_and_propagates() {
    // 零丢失失败语义：执行不了的 op 不落日志、错误上抛（重投递会重试）；
    // 此前已应用的 op 保持已应用。挂起队列（不阻塞其余重放）由 #856 承接。
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    // op1：合法（两端账户齐备）；op2：引用仅 A 端有的账户（B 端外键依赖失败，
    // 真实世界对应「参考数据未同步」）。
    seed_account(&conn_a, "acc-a-only", "A 独有账户", "cash", "CNY", 0);
    behavior::create(&conn_a, make_expense("acc-1", 10000, "好的")).unwrap();
    behavior::create(&conn_a, make_expense("acc-a-only", 500, "坏引用")).unwrap();
    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 2);

    apply_ops(&conn_b, &ops).unwrap_err();
    // 失败 op 不落日志（重投递重试，不静默丢弃）；
    assert!(
        read_ops(&conn_b)
            .unwrap()
            .iter()
            .all(|op| op.op_id != ops[1].op_id)
    );
    // 此前已应用的 op 保持已应用，且对应数据在库。
    assert!(
        read_ops(&conn_b)
            .unwrap()
            .iter()
            .any(|op| op.op_id == ops[0].op_id),
        "失败前的 op 保持已应用"
    );
    let DomainCommand::Transaction(crate::transaction::TransactionCommand::Create {
        id: applied_id,
        ..
    }) = &ops[0].command
    else {
        panic!("应为 create 命令");
    };
    assert!(
        read_transaction(&conn_b, applied_id).is_some(),
        "已应用 op 的数据在库"
    );
    // 幂等重试：仅重投失败 op 仍失败（非「已应用却报错」的假失败）。
    assert!(apply_ops(&conn_b, &ops[1..]).is_err());
}
