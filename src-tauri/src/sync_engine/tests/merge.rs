//! 双端合并语义（issue #856 / ADR-0091 决策 4/5/6）：LWW、OccurrenceKey 防双扣、
//! ParkedOp 挂起队列与 schema 偏斜双向挂起。
//!
//! 双端场景 = 同进程两个引擎实例 + 内存假 Transport（wire 形态 JSON 字符串传递，
//! #859 真通道接线前的合成验证）；造数走公开写入口（测试侧），重放复用既有
//! 行为编排入口，断言权威 = 同步引擎公开接口。

use super::super::{ApplyReport, DomainCommand, OpOutcome, parked_ops, read_ops};
use super::common::{
    make_expense, read_occurrence, read_transaction, seed_device, seed_occurrence, seed_plan,
    wire_in, wire_out,
};
use crate::scheduled_transactions::{execute_occurrence, occurrence_transaction_id};
use crate::test_support::{self, assert_balance_cache_matches_realtime, seed_account};
use crate::transaction::behavior;

/// 并发修改同一笔交易：两端按全序取序末者（LWW），输者的 op 落日志可追溯。
#[test]
fn concurrent_same_field_edits_converge_to_order_last() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    // 共同基底：A 创建一笔交易，两端一致持有同一实体。
    let id = behavior::create(&conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    // 两端并发改同一字段（备注）：A 端时钟已到 2，B 端本地首写时钟为 1，
    // 全序判 A 的修改为序末者 → LWW 取 A。
    behavior::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();
    behavior::update(&conn_b, &id, make_expense("acc-1", 10000, "B 改")).unwrap();

    let reports_b = wire_in(&conn_b, &wire_out(&conn_a));
    let reports_a = wire_in(&conn_a, &wire_out(&conn_b));

    // 两端收敛到序末者（A 的修改）；余额缓存自洽。
    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(expected.note.as_deref(), Some("A 改"), "全序末者胜");
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);

    // 输者（B 的修改）不执行但落日志：两端日志一致且均可追溯。
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
    assert!(
        read_ops(&conn_a).unwrap().iter().any(|op| matches!(
            &op.command,
            DomainCommand::Transaction(crate::transaction::TransactionCommand::Update { row, .. })
                if row.note.as_deref() == Some("B 改")
        )),
        "输者的 op 仍可审计"
    );

    // 逐条报告：B 执行序末者，A 端输者标记 Superseded。假通道投递对端全量日志
    // （含对端已重放过的本端 op），故各自末尾多一条已知 op 的 Skipped。
    assert_eq!(
        outcomes(&reports_a),
        vec![
            OpOutcome::Skipped,
            OpOutcome::Superseded,
            OpOutcome::Skipped
        ],
        "A 端：A 的旧 op 跳过，B 的并发修改被 LWW 压制"
    );
    assert_eq!(
        outcomes(&reports_b),
        vec![OpOutcome::Skipped, OpOutcome::Applied],
        "B 端：旧 op 跳过，序末者执行"
    );
    assert_balance_cache_matches_realtime(&conn_a);
    assert_balance_cache_matches_realtime(&conn_b);
}

/// 并发「删除 vs 修改」：修改为序末者时，删除端不自动复活（park 承接裁决）。
///
/// v1 边界钉子：同实体 LWW 判修改胜，但删除端的重放命中软删行（本地语义
/// 不可改已删行）→ 挂起队列待用户裁决，不静默复活；胜者端保持修改后状态。
#[test]
fn delete_vs_concurrent_update_never_resurrects_loser_side_parks() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    // 固定 DeviceId：dev-a < dev-b，全序确定（A 的修改为序末者）。
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let id = behavior::create(&conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    behavior::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();
    behavior::delete(&conn_b, &id).unwrap();

    wire_in(&conn_b, &wire_out(&conn_a));
    wire_in(&conn_a, &wire_out(&conn_b));

    // 胜者端（A）：保持修改后的存活状态；输者端（B）：保持删除、不复活。
    assert_eq!(
        read_transaction(&conn_a, &id).unwrap().note.as_deref(),
        Some("A 改")
    );
    assert_eq!(read_transaction(&conn_b, &id).unwrap().is_deleted, 1);
    // B 端修改 op 挂起待裁决（码化原因：交易不存在），不静默丢弃。
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].code, "transaction.not-found");
}

fn outcomes(reports: &[ApplyReport]) -> Vec<OpOutcome> {
    reports.iter().map(|r| r.outcome.clone()).collect()
}

// ---------------------------------------------------------------------------
// OccurrenceKey 防双扣（ADR-0091 决策 5）：期次身份 = plan_id + 计划日期，
// 落地身份为键派生的确定性交易 id——两台设备同时自动执行同一期只落一次。
// ---------------------------------------------------------------------------

const PLAN: &str = "plan-1";
const FEB: &str = "2026-02-01";

/// 两端同时自动执行同一计划期次：互换后该期次只落一次，两端收敛同一行。
#[test]
fn concurrent_occurrence_execution_lands_exactly_once() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-1", "现金", "cash", "CNY", 0);
        seed_plan(conn, PLAN, "acc-1", 3000);
    }
    // 期次行 id 两端刻意不同：期次身份是 (plan_id, 计划日期)，不是本地行 id。
    seed_occurrence(&conn_a, "occ-a", PLAN, FEB, 3000);
    seed_occurrence(&conn_b, "occ-b", PLAN, FEB, 3000);

    // 双端同时自动执行同一期（各自落地 + 各自产出 op）。
    let landed_a = execute_occurrence(&conn_a, "occ-a").unwrap();
    let landed_b = execute_occurrence(&conn_b, "occ-b").unwrap();
    assert_eq!(
        landed_a, landed_b,
        "同一期次的落地身份跨端一致（OccurrenceKey 派生）"
    );

    // 互换日志：各自对端 op 按期次幂等命中，不产生第二笔。
    let reports_b = wire_in(&conn_b, &wire_out(&conn_a));
    let reports_a = wire_in(&conn_a, &wire_out(&conn_b));
    assert!(
        reports_a
            .iter()
            .chain(&reports_b)
            .any(|r| r.outcome == OpOutcome::Deduped),
        "对端期次 op 按键去重"
    );

    // 该期次只落一次：两端恰好一笔、同一行；期次完成并回填同一交易。
    for (conn, occ) in [(&conn_a, "occ-a"), (&conn_b, "occ-b")] {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "该期次只落一次");
        assert_eq!(
            read_transaction(conn, &landed_a).unwrap().amount_cents,
            3000
        );
        assert_eq!(
            read_occurrence(conn, occ).unwrap(),
            ("completed".into(), Some(landed_a.clone())),
            "期次完成并回填落地交易"
        );
    }
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
    assert_balance_cache_matches_realtime(&conn_a);
    assert_balance_cache_matches_realtime(&conn_b);
}

/// 单端自动执行：对端重放后落地同笔交易并完成对端本地期次；对端期次此后
/// 不可重复执行（不双扣）。
#[test]
fn single_side_execution_replays_and_completes_peer_occurrence() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-1", "现金", "cash", "CNY", 0);
        seed_plan(conn, PLAN, "acc-1", 3000);
    }
    seed_occurrence(&conn_a, "occ-a", PLAN, FEB, 3000);
    seed_occurrence(&conn_b, "occ-b", PLAN, FEB, 3000);

    let landed = execute_occurrence(&conn_a, "occ-a").unwrap();
    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(outcomes(&reports), vec![OpOutcome::Applied]);

    // B 端：交易落地（源端折算随行）、本地期次完成并回填。
    assert_eq!(
        read_transaction(&conn_b, &landed).unwrap().amount_cents,
        3000
    );
    assert_eq!(
        read_occurrence(&conn_b, "occ-b").unwrap(),
        ("completed".into(), Some(landed.clone()))
    );
    assert_balance_cache_matches_realtime(&conn_b);

    // 对端期次已完成：重复执行被状态守卫拒绝（不双扣）。
    assert!(execute_occurrence(&conn_b, "occ-b").is_err());
    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

/// 落地已存在而本地期次仍 pending（期次行晚于落地到达）：执行走落地已存在
/// 路径——只回填完成、不插第二笔。
#[test]
fn execution_with_already_landed_transaction_completes_without_second_row() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-1", "现金", "cash", "CNY", 0);
        seed_plan(conn, PLAN, "acc-1", 3000);
    }
    seed_occurrence(&conn_a, "occ-a", PLAN, FEB, 3000);

    // A 执行并同步到 B（B 尚无对应期次行）：交易落地，无本地期次可回填。
    let landing = execute_occurrence(&conn_a, "occ-a").unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert!(read_occurrence(&conn_b, "occ-late").is_none());

    // 期次行晚于落地到达（延迟同步/窗口展开差异）：执行发现落地已存在，
    // 只回填完成，不产生第二笔。
    seed_occurrence(&conn_b, "occ-late", PLAN, FEB, 3000);
    let again = execute_occurrence(&conn_b, "occ-late").unwrap();
    assert_eq!(again, landing, "落地身份一致，不插第二笔");
    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "同一期次只有一笔交易");
    assert_eq!(
        read_occurrence(&conn_b, "occ-late").unwrap(),
        ("completed".into(), Some(landing)),
        "期次完成并回填既有落地"
    );
    assert_balance_cache_matches_realtime(&conn_b);
}

/// 期次触发命令的实体指向为 None：冲突域在 OccurrenceKey，不参与同实体 LWW。
#[test]
fn occurrence_command_has_no_entity_subject() {
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    seed_plan(&conn, PLAN, "acc-1", 3000);
    seed_occurrence(&conn, "occ-a", PLAN, FEB, 3000);
    execute_occurrence(&conn, "occ-a").unwrap();
    let ops = read_ops(&conn).unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].command.entity(), "scheduled");
    assert!(ops[0].command.subject().is_none());
    let DomainCommand::Scheduled(cmd) = &ops[0].command else {
        panic!("应为期次触发命令");
    };
    let (plan_id, scheduled_date) = match cmd {
        crate::scheduled_transactions::ScheduledCommand::ExecuteOccurrence {
            plan_id,
            scheduled_date,
            ..
        } => (plan_id, scheduled_date),
    };
    assert_eq!(plan_id, PLAN);
    assert_eq!(scheduled_date, FEB);
    assert_eq!(
        occurrence_transaction_id(plan_id, scheduled_date),
        cmd.landing_transaction_id(),
        "落地身份由 OccurrenceKey 派生"
    );
}
