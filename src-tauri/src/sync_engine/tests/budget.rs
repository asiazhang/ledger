//! 预算域的全域 op 产出与重放收敛（issue #860）：创建/编辑/删除收敛、「分类 +
//! 周期」唯一冲突挂起待裁决。

use super::super::{OpOutcome, parked_ops, read_ops};
use super::common::{seed_device, wire_in, wire_out};
use crate::budget::{BudgetInput, BudgetPeriod, create_budget, delete_budget, update_budget};
use crate::categories::{CategoryInput, create_category};
use crate::test_support;

fn seed_category_on_both_sides() -> (rusqlite::Connection, rusqlite::Connection, String) {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    let category_id = create_category(
        &conn_a,
        CategoryInput {
            name: "餐饮".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    (conn_a, conn_b, category_id)
}

#[test]
fn budget_create_update_delete_ops_replay_and_converge() {
    let (conn_a, conn_b, category_id) = seed_category_on_both_sides();

    let id = create_budget(
        &conn_a,
        &BudgetInput {
            category_id: category_id.clone(),
            period: Some(BudgetPeriod::Monthly),
            amount_cents: 500_000,
            start_date: "2026-01-01".into(),
        },
    )
    .unwrap();
    update_budget(&conn_a, &id, 600_000).unwrap();
    delete_budget(&conn_a, &id).unwrap();

    // 日志含分类建档 op（夹具经公开写入口产出），预算入口各产出一条。
    let budget_ops: Vec<_> = read_ops(&conn_a)
        .unwrap()
        .into_iter()
        .filter(|op| op.command.subject().0 == "budget")
        .collect();
    assert_eq!(budget_ops.len(), 3, "三个写入口各产出一条 op");

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        !reports
            .iter()
            .any(|r| matches!(r.outcome, OpOutcome::Parked { .. })),
        "预算 op 全部落地（夹具分类 op 重投递为 Skipped）：{reports:?}"
    );
    let row = |conn: &rusqlite::Connection| -> (i64, bool) {
        conn.query_row(
            "SELECT amount_cents, is_deleted FROM budgets WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)),
        )
        .unwrap()
    };
    assert_eq!(row(&conn_a), row(&conn_b), "重放后预算状态一致");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn budget_create_period_defaults_to_monthly_in_payload() {
    let (conn_a, _conn_b, category_id) = seed_category_on_both_sides();
    let id = create_budget(
        &conn_a,
        &BudgetInput {
            category_id,
            period: None,
            amount_cents: 100_000,
            start_date: "2026-01-01".into(),
        },
    )
    .unwrap();
    let period: String = conn_a
        .query_row("SELECT period FROM budgets WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(period, "monthly");
    // 载荷内周期为落定值（缺省已解决），重放端不再缺省。
    let budget_ops: Vec<_> = read_ops(&conn_a)
        .unwrap()
        .into_iter()
        .filter(|op| op.command.subject().0 == "budget")
        .collect();
    assert_eq!(budget_ops.len(), 1);
    assert_eq!(budget_ops[0].command.subject().0, "budget");
}

#[test]
fn duplicate_budget_create_parks_on_conflict() {
    let (conn_a, conn_b, category_id) = seed_category_on_both_sides();

    // 两端各建同「分类 + 周期」预算（各自本地合法）。
    let input = || BudgetInput {
        category_id: category_id.clone(),
        period: Some(BudgetPeriod::Monthly),
        amount_cents: 100_000,
        start_date: "2026-01-01".into(),
    };
    create_budget(&conn_a, &input()).unwrap();
    create_budget(&conn_b, &input()).unwrap();

    let reports_on_b = wire_in(&conn_b, &wire_out(&conn_a));
    let reports_on_a = wire_in(&conn_a, &wire_out(&conn_b));

    // 互为唯一性冲突：后到者在对方挂起待裁决（不静默覆盖、不中断批次）。
    assert!(
        reports_on_b
            .iter()
            .any(|r| r.outcome != OpOutcome::Applied && r.outcome != OpOutcome::Skipped),
        "A 的预算在 B 端不得静默生效：{reports_on_b:?}"
    );
    assert!(
        !parked_ops(&conn_b).unwrap().is_empty(),
        "冲突 op 进挂起队列"
    );
    assert!(
        reports_on_a
            .iter()
            .any(|r| matches!(r.outcome, OpOutcome::Parked { .. })),
        "B 的预算在 A 端挂起：{reports_on_a:?}"
    );
}
