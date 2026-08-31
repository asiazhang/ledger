//! 期次详情弹窗（issue #205）：详情返回 failed 期次与已完成期次列表；
//! 期次展开与失败重试。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::scheduled_transactions::{
    ScheduledTransactionDetail, expand_occurrences, get_plan_detail,
};

use crate::world::LedgerWorld;

use super::common::execute_occurrence_step;

// ---------------------------------------------------------------------------
// 期次详情弹窗（issue #205）：详情返回 failed 期次与已完成期次列表；期次展开
// ---------------------------------------------------------------------------

/// 把最近计划最早的一条 pending 期次置为 failed（当前引擎失败路径在 CAS 前
/// 返回、保持 pending 可重试；failed 为 ADR-0001 预留状态，此处直接构造
/// 以驱动详情返回与重试门控的断言）。
#[when(expr = "将最近计划最早的一条待执行期次置为失败")]
fn mark_first_pending_failed(world: &mut LedgerWorld) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let occ_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM scheduled_transaction_occurrences \
             WHERE scheduled_transaction_id=?1 AND status='pending' AND is_deleted=0 \
             ORDER BY scheduled_date ASC LIMIT 1",
            params![plan_id],
            |r| r.get(0),
        )
        .expect("计划应已有 pending 期次");
    world_conn!(world)
        .execute(
            "UPDATE scheduled_transaction_occurrences SET status='failed', updated_at=?2, \
             version=version+1 WHERE id=?1",
            params![occ_id, tauri_app_lib::db::now_iso()],
        )
        .unwrap();
    world.last_occurrence_id = Some(occ_id);
}

/// 查询最近计划的详情（走 get_plan_detail 命令体）。
#[when(expr = "查询该计划详情")]
fn query_plan_detail(world: &mut LedgerWorld) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    world.last_detail =
        Some(get_plan_detail(&world_conn!(world), &plan_id).expect("查询计划详情失败"));
}

fn last_detail(world: &LedgerWorld) -> &ScheduledTransactionDetail {
    world.last_detail.as_ref().expect("尚未查询计划详情")
}

#[then(expr = "详情应含 {int} 条待执行期次")]
fn assert_detail_pending(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        last_detail(world).pending_occurrences.len(),
        expected,
        "待执行期次条数不符"
    );
}

#[then(expr = "详情期次总数应为 {int}")]
fn assert_detail_occurrence_total(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        last_detail(world).occurrences.len(),
        expected,
        "期次总数不符"
    );
}

#[then(expr = "详情状态为 {string} 的期次应有 {int} 条")]
fn assert_detail_status_count(world: &mut LedgerWorld, status: String, expected: usize) {
    let n = last_detail(world)
        .occurrences
        .iter()
        .filter(|o| o.status == status)
        .count();
    assert_eq!(n, expected, "状态为 {status} 的期次条数不符");
}

#[then(expr = "详情状态为 {string} 的期次日期应为 {string}")]
fn assert_detail_status_date(world: &mut LedgerWorld, status: String, expected: String) {
    let dates: Vec<String> = last_detail(world)
        .occurrences
        .iter()
        .filter(|o| o.status == status)
        .map(|o| o.scheduled_date.clone())
        .collect();
    assert_eq!(dates, vec![expected], "状态为 {status} 的期次日期不符");
}

/// 重试最近计划的 failed 期次（走 execute_occurrence 命令体，与弹窗重试同一缝）。
#[when(expr = "重试该失败期次")]
fn retry_failed_occurrence(world: &mut LedgerWorld) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let occ_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM scheduled_transaction_occurrences \
             WHERE scheduled_transaction_id=?1 AND status='failed' AND is_deleted=0 \
             ORDER BY scheduled_date ASC LIMIT 1",
            params![plan_id],
            |r| r.get(0),
        )
        .expect("计划应已有 failed 期次");
    execute_occurrence_step(world, &occ_id);
}

/// 展开最近计划的期次（走 expand_occurrences 命令体）。
#[when(expr = "展开该计划期次")]
fn expand_plan_occurrences(world: &mut LedgerWorld) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let ids = world
        .db
        .write(|conn| expand_occurrences(conn, &plan_id))
        .expect("期次展开失败");
    assert!(!ids.is_empty(), "active 计划展开应生成新期次");
}
