//! 定时计划域的全域 op 产出与重放收敛（issue #860）：建档（期次本地展开）、
//! 状态变更、订阅编辑、期次展开；依赖缺失挂起后经重投递自愈。

use super::super::{OpOutcome, parked_ops, read_ops};
use super::common::{seed_device, wire_in, wire_out};
use crate::scheduled_transactions::{
    RecurrenceType, ScheduledKind, ScheduledStatus, create_plan, expand_occurrences,
    update_plan_status, update_subscription,
};
use crate::test_support;
use crate::test_support::seed_account;

fn plan_input(
    account_id: &str,
    start_date: &str,
) -> crate::scheduled_transactions::CreateScheduledInput {
    crate::scheduled_transactions::CreateScheduledInput {
        kind: ScheduledKind::Subscription,
        account_id: account_id.into(),
        category_id: None,
        amount_cents: 3_000,
        currency_code: "CNY".into(),
        recurrence_type: RecurrenceType::Monthly,
        recurrence_interval: 1,
        recurrence_day: None,
        start_date: start_date.into(),
        note: Some("视频会员".into()),
        merchant_id: None,
        policy_id: None,
        total_amount_cents: None,
        total_occurrences: None,
        to_account_id: None,
    }
}

/// 两端计划业务状态快照：(status, note, 期次日期集合)——期次行 id 各端独立生成，
/// 不作身份（Occurrence 语义）。
fn plan_snapshot(
    conn: &rusqlite::Connection,
    plan_id: &str,
) -> (String, Option<String>, Vec<String>) {
    let (status, note) = conn
        .query_row(
            "SELECT status, note FROM scheduled_transactions WHERE id=?1",
            [plan_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let mut dates: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT scheduled_date FROM scheduled_transaction_occurrences \
                 WHERE scheduled_transaction_id=?1 AND is_deleted=0 ORDER BY scheduled_date",
            )
            .unwrap();
        let rows = stmt.query_map([plan_id], |r| r.get(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };
    dates.sort();
    (status, note, dates)
}

#[test]
fn plan_lifecycle_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let plan_id = create_plan(&conn_a, plan_input("acc-1", "2026-01-01")).unwrap();
    // 展开需在 active 状态执行（paused 计划不展开，计划域语义）；随后暂停与编辑。
    expand_occurrences(&conn_a, &plan_id).unwrap();
    update_plan_status(&conn_a, &plan_id, ScheduledStatus::Paused).unwrap();
    update_subscription(
        &conn_a,
        crate::scheduled_transactions::UpdateSubscriptionInput {
            id: plan_id.clone(),
            account_id: "acc-1".into(),
            category_id: None,
            note: Some("视频会员·年付档".into()),
            merchant_id: None,
            amount_cents: false,
            total_amount_cents: false,
        },
    )
    .unwrap();

    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 4, "建档/状态/编辑/展开各一条：{ops:?}");
    assert!(ops.iter().all(|op| op.command.subject().0 == "scheduled"));

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        reports.iter().all(|r| r.outcome == OpOutcome::Applied),
        "重放全部落地（期次展开按 B 端现状重推）：{reports:?}"
    );
    let snapshot_a = plan_snapshot(&conn_a, &plan_id);
    let snapshot_b = plan_snapshot(&conn_b, &plan_id);
    assert_eq!(snapshot_a.0, "paused");
    assert_eq!(snapshot_a, snapshot_b, "计划与期次（按日期集合）收敛");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());

    // 重复投递：全部 Skipped。
    let again = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(again.iter().all(|r| r.outcome == OpOutcome::Skipped));
}

#[test]
fn concurrent_plan_status_edits_converge_to_order_last() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    // 计划在两端等量种子（夹具不产 op，两端时钟同为 0）：并发状态变更时钟相同，
    // 全序 tiebreak 落 DeviceId 字典序（dev-a < dev-b，B 末）。
    test_support::seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    super::common::seed_plan(&conn_a, "plan-1", "acc-1", 3_000);
    super::common::seed_plan(&conn_b, "plan-1", "acc-1", 3_000);

    // 并发：A 暂停、B 取消（B 端计划为 active，转换合法）。
    update_plan_status(&conn_a, "plan-1", ScheduledStatus::Paused).unwrap();
    update_plan_status(&conn_b, "plan-1", ScheduledStatus::Cancelled).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    wire_in(&conn_a, &wire_out(&conn_b));

    // 全序 (clock, device_id)：同为 clock 1 时 dev-b 在末 → 两端收敛 cancelled。
    assert_eq!(plan_snapshot(&conn_a, "plan-1").0, "cancelled");
    assert_eq!(plan_snapshot(&conn_b, "plan-1").0, "cancelled");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn plan_create_parks_until_account_arrives() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    // 账户经公开写入口创建（产 account op）；先只投递 scheduled op。
    let account_id = crate::accounts::create_account(
        &conn_a,
        crate::accounts::AccountInput {
            name: "扣款卡".into(),
            kind: crate::accounts::AccountType::Bank,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
        },
    )
    .unwrap();
    let plan_id = create_plan(&conn_a, plan_input(&account_id, "2026-01-01")).unwrap();

    let scheduled_wire: Vec<String> = wire_out(&conn_a)
        .into_iter()
        .filter(|raw| raw.contains("\"entity\":\"scheduled\""))
        .collect();
    assert_eq!(scheduled_wire.len(), 1);
    let reports = wire_in(&conn_b, &scheduled_wire);
    assert!(
        reports
            .iter()
            .any(|r| matches!(r.outcome, OpOutcome::Parked { .. })),
        "扣款账户缺失 → 建档挂起：{reports:?}"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);

    // 依赖方补齐后重投递：账户先建，计划建档重放成功即出队。
    let full_reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        full_reports.iter().any(|r| r.outcome == OpOutcome::Applied),
        "重投递后计划落地：{full_reports:?}"
    );
    assert!(parked_ops(&conn_b).unwrap().is_empty());
    assert_eq!(
        plan_snapshot(&conn_a, &plan_id).0,
        plan_snapshot(&conn_b, &plan_id).0,
        "计划状态收敛（active）"
    );
}
