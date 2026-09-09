//! 保险域的全域 op 产出与重放收敛（issue #860）：保司字典 + 保单档案；依赖缺失
//! 挂起后经重投递自愈（ParkedOp 出队语义）。

use super::super::{DomainCommand, OpOutcome, parked_ops, read_ops};
use super::common::{seed_device, wire_in, wire_out};
use crate::policy::{
    InsurerInput, PolicyCommand, PolicyInput, create_insurer, create_policy, delete_policy,
    update_policy,
};
use crate::test_support;

fn policy_input(insurer_id: &str) -> PolicyInput {
    PolicyInput {
        insurer_id: insurer_id.into(),
        policy_number: "P-2026-001".into(),
        product_name: "百万医疗".into(),
        start_date: "2026-01-01".into(),
        end_date: Some("2026-12-31".into()),
        coverage_amount_cents: Some(2_000_000),
        coverage_currency_code: Some("CNY".into()),
        note: None,
    }
}

#[test]
fn insurer_and_policy_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let insurer_id = create_insurer(
        &conn_a,
        InsurerInput {
            name: "青枫人寿".into(),
        },
    )
    .unwrap();
    let policy_id = create_policy(&conn_a, policy_input(&insurer_id), &mut || {}).unwrap();
    update_policy(
        &conn_a,
        &policy_id,
        PolicyInput {
            product_name: "百万医疗险".into(),
            ..policy_input(&insurer_id)
        },
        &mut || {},
    )
    .unwrap();
    delete_policy(&conn_a, &policy_id, &mut || {}).unwrap();

    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(
        ops.len(),
        4,
        "保司创建 + 保单 create/update/delete：{ops:?}"
    );
    assert_eq!(ops[0].command.entity(), "insurer");
    assert!(ops[1..].iter().all(|op| op.command.entity() == "policy"));

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Applied));

    let insurer_name: String = conn_b
        .query_row(
            "SELECT name FROM insurers WHERE id=?1",
            [&insurer_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(insurer_name, "青枫人寿");
    let row = |conn: &rusqlite::Connection| -> (String, Option<i64>, bool) {
        conn.query_row(
            "SELECT product_name, coverage_amount_cents, is_deleted FROM policies WHERE id=?1",
            [&policy_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)),
        )
        .unwrap()
    };
    assert_eq!(
        row(&conn_a),
        row(&conn_b),
        "重放后保单状态一致（编辑 + 软删）"
    );
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());

    // 重复投递：全部 Skipped。
    let again = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(again.iter().all(|r| r.outcome == OpOutcome::Skipped));
}

#[test]
fn policy_create_parks_until_insurer_arrives() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let insurer_id = create_insurer(
        &conn_a,
        InsurerInput {
            name: "南山财险".into(),
        },
    )
    .unwrap();
    let policy_id = create_policy(&conn_a, policy_input(&insurer_id), &mut || {}).unwrap();

    // 先只投递保单 op（依赖的保司 op 未达）：保司在用校验失败 → 挂起，不落日志。
    let policy_wire: Vec<String> = wire_out(&conn_a)
        .into_iter()
        .filter(|raw| raw.contains("\"entity\":\"policy\""))
        .collect();
    assert_eq!(policy_wire.len(), 1);
    let reports = wire_in(&conn_b, &policy_wire);
    assert!(
        reports
            .iter()
            .any(|r| matches!(r.outcome, OpOutcome::Parked { .. })),
        "保司缺失 → 挂起待裁决：{reports:?}"
    );
    assert!(read_ops(&conn_b).unwrap().is_empty(), "挂起 op 不落日志");
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);

    // 依赖方补齐后重投递：保司先建，保单 op 重放成功即出队（幂等覆盖挂起行）。
    let full_reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        full_reports.iter().any(|r| r.outcome == OpOutcome::Applied),
        "重投递后保单 op 落地：{full_reports:?}"
    );
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    let landed = read_ops(&conn_b)
        .unwrap()
        .iter()
        .any(|op| matches!(&op.command, DomainCommand::Policy(PolicyCommand::Create { id, .. }) if id == &policy_id));
    assert!(landed, "保单创建 op 已落日志");
}
