//! 多语句读闭包的快照一致性探针（issue #1702）：写提交落在语句之间时，同屏
//! 口径必须仍互相自洽——计划详情的完成计数与期次明细同时点、计划列表的
//! core × 扩展两段 join 同时点、订阅花费的行清单与推算成本同时点。探针机制
//! 见 `tauri_app_lib::test_support::snapshot_probe`。

use chrono::NaiveDate;

use super::super::*;
use super::common::{create_subscription, create_transfer_plan, first_pending_occurrence};
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};
use tauri_app_lib::test_support::{ScratchDir, open_file, seed_account};

/// 计划详情的完成计数与期次明细必须同快照（issue #1702）：pending 列表、
/// 「已完成 N 期 / 已还金额」计数与全量期次列表是多段语句（页与总数形态）。
/// 探针在计数读取开始前于另一连接把一笔 pending 期次置为 completed——
/// - 读闭包无快照保护（红）：pending 列表读旧（含该笔），计数与明细读新
///   （该笔已被计为 completed）——同一期次同屏既在 pending 又被计为已完成；
/// - 读闭包收进读事务（绿）：注入写被挡住，计数与明细同见一套数。
#[test]
fn plan_detail_count_and_occurrences_share_one_snapshot() {
    let dir = ScratchDir::new("scheduled-detail-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-snap", "订阅户", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc-snap", "CNY", 10_000, None);
    let occ_id = first_pending_occurrence(&conn, &plan_id);
    execute_occurrence(&conn, &occ_id).unwrap();

    let completed_rows = |d: &ScheduledTransactionDetail| {
        d.occurrences
            .iter()
            .filter(|o| o.status == "completed")
            .count() as i64
    };
    let before = get_plan_detail(&conn, &plan_id).unwrap();
    assert_eq!(
        before.completed_occurrences, 1,
        "种子应产出 1 期已完成（否则口径断言空转）"
    );
    assert_eq!(
        completed_rows(&before),
        1,
        "明细 completed 行数应与计数一致（否则口径断言空转）"
    );

    // 探针：完成计数读取（`COALESCE(SUM(amount_cents),0) FROM
    // scheduled_transaction_occurrences`，全闭包唯一命中）开始前，另一连接
    // 提交期次状态翻转。
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "COALESCE(SUM(amount_cents),0) FROM scheduled_transaction_occurrences",
        &["UPDATE scheduled_transaction_occurrences SET status='completed' WHERE status='pending'"],
    );
    let after = get_plan_detail(&conn, &plan_id).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中完成计数读取（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(
        before.completed_occurrences, after.completed_occurrences,
        "完成计数必须与基线同时点"
    );
    assert_eq!(
        before.completed_amount_cents, after.completed_amount_cents,
        "已还金额必须与基线同时点"
    );
    assert_eq!(
        after.completed_occurrences,
        completed_rows(&after),
        "计数与明细必须同快照（页与总数错位即矛盾）"
    );
    let pending_ids: std::collections::HashSet<&str> = after
        .pending_occurrences
        .iter()
        .map(|o| o.id.as_str())
        .collect();
    let overlap = after
        .occurrences
        .iter()
        .filter(|o| o.status == "completed" && pending_ids.contains(o.id.as_str()))
        .count();
    assert_eq!(
        overlap, 0,
        "同一期次不得同屏既在 pending 列表又被计入已完成（列表与计数异快照即矛盾）"
    );
}

/// 计划列表的 core 行与扩展字段必须同快照（issue #1702）：列表先读全部 core
/// 行、再逐计划读 ext 表（两段 join 形态）。探针在转账扩展读取开始前于另一
/// 连接改写转入账户——
/// - 读闭包无快照保护（红）：行内 core 与 ext 异版本（旧 core 配新 ext）；
/// - 读闭包收进读事务（绿）：注入写被挡住，行内两段同见一套数。
#[test]
fn plan_list_core_and_ext_share_one_snapshot() {
    let dir = ScratchDir::new("scheduled-list-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-snap-from", "转出户", "cash", "CNY", 0);
    seed_account(&conn, "acc-snap-to", "转入户", "cash", "CNY", 0);
    create_transfer_plan(&conn, "acc-snap-from", "acc-snap-to", 50_000);

    let before = list_plans(&conn).unwrap();
    assert_eq!(before.len(), 1, "种子应产出恰好一行（否则口径断言空转）");
    assert_eq!(
        before[0].to_account_id.as_deref(),
        Some("acc-snap-to"),
        "种子转入账户应就位（否则口径断言空转）"
    );

    // 探针：转账扩展读取（`FROM scheduled_transfer_plans`，全闭包唯一命中）
    // 开始前，另一连接改写 ext 行。
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM scheduled_transfer_plans",
        &["UPDATE scheduled_transfer_plans SET to_account_id = 'acc-snap-from'"],
    );
    let after = list_plans(&conn).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中扩展读取（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(
        before[0].to_account_id, after[0].to_account_id,
        "行内扩展字段必须与基线同时点（core 读旧、ext 读新即漂移）"
    );
    assert_eq!(
        before[0].core.updated_at, after[0].core.updated_at,
        "行内 core 侧不应漂移"
    );
}

/// 订阅花费总览的行清单与推算成本必须同快照（issue #1702）：「行内花费与顶层
/// 汇总同源」依赖同一快照——逐月聚合、订阅行、推算成本（计划金额 × 汇率）
/// 是多段语句。探针在推算成本的计划读取开始前于另一连接把订阅金额翻倍——
/// - 读闭包无快照保护（红）：行清单读旧（金额 10000）而推算成本读新
///   （按 20000 推算），同屏口径互相矛盾；
/// - 读闭包收进读事务（绿）：注入写被挡住，行与推算同见一套数。
#[test]
fn subscription_spend_rows_and_projection_share_one_snapshot() {
    let dir = ScratchDir::new("scheduled-spend-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-snap-spend", "订阅户", "cash", "CNY", 0);
    create_subscription(&conn, "acc-snap-spend", "CNY", 10_000, None);
    let today = NaiveDate::parse_from_str("2026-02-01", "%Y-%m-%d").unwrap();

    let before = query_subscription_spend(&conn, today).unwrap();
    assert_eq!(
        before.rows.len(),
        1,
        "种子应产出恰好一行订阅（否则口径断言空转）"
    );
    assert_eq!(
        before.rows[0].amount_cents, 10_000,
        "种子行金额应就位（否则口径断言空转）"
    );
    assert_eq!(
        before.projected_month_native_cents, 10_000,
        "推算月成本应等于计划金额（否则口径断言空转）"
    );

    // 探针：推算成本的计划读取（`status = 'active'`，全闭包唯一命中）开始前，
    // 另一连接提交金额翻倍。
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "status = 'active'",
        &[
            "UPDATE scheduled_transactions SET amount_cents = amount_cents * 2 WHERE kind = 'subscription'",
        ],
    );
    let after = query_subscription_spend(&conn, today).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中推算成本读取（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(
        before
            .rows
            .iter()
            .map(|r| (
                r.plan_id.as_str(),
                r.status.as_str(),
                r.amount_cents,
                r.currency_code.as_str()
            ))
            .collect::<Vec<_>>(),
        after
            .rows
            .iter()
            .map(|r| (
                r.plan_id.as_str(),
                r.status.as_str(),
                r.amount_cents,
                r.currency_code.as_str()
            ))
            .collect::<Vec<_>>(),
        "订阅行清单必须与基线同时点"
    );
    assert_eq!(
        before.projected_month_native_cents, after.projected_month_native_cents,
        "推算月成本必须与基线同时点（行金额读旧、推算读新即同屏矛盾）"
    );
    assert_eq!(
        before.projected_year_native_cents, after.projected_year_native_cents,
        "推算年成本必须与基线同时点"
    );
    assert_eq!(
        before
            .months
            .iter()
            .map(|m| (m.month.as_str(), m.native_cents))
            .collect::<Vec<_>>(),
        after
            .months
            .iter()
            .map(|m| (m.month.as_str(), m.native_cents))
            .collect::<Vec<_>>(),
        "逐月花费必须与基线同时点"
    );
}
