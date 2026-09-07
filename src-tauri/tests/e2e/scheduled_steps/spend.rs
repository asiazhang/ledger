//! 订阅实际花费口径（issue #160，ADR-0023 决策二）：多周期订阅夹具、
//! 执行前 N 期 / 取消 / 暂停、花费总览断言。
//!
//! #762 迁移：计划创建收编 L1 订阅工厂（周期/备注为冷字段覆盖）、写入收编 L2
//! 计划动词；生命周期变更（取消/暂停）经生命周期动词走既有命令形态域函数
//! `update_plan_status`，不为测试开旁路。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::scheduled_transactions::{
    CreateScheduledInput, RecurrenceType, ScheduledStatus, SubscriptionSpendOverview,
    query_subscription_spend,
};

use crate::step_inputs::subscription_plan_input;
use crate::step_verbs::{create_plan_verb, update_plan_status_verb};
use crate::world::LedgerWorld;

use super::common::execute_occurrence_step;

// ---------------------------------------------------------------------------
// 订阅花费——实际花费口径（issue #160，ADR-0023 决策二）
// ---------------------------------------------------------------------------

#[when(
    expr = "创建订阅计划 金额 {int} 币种 {string} 账户 {string} 周期 {string} 起始日期 {string} 备注 {string}"
)]
fn create_subscription_plan_with_recurrence(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    recurrence: String,
    start: String,
    note: String,
) {
    let recurrence_type: RecurrenceType = recurrence
        .parse()
        .expect("周期应为 daily/weekly/monthly/yearly");
    let account_id = world.account_id(&account);
    create_plan_verb(
        world,
        CreateScheduledInput {
            recurrence_type,
            note: Some(note),
            ..subscription_plan_input(amount, &account_id, &currency, &start)
        },
    );
}

/// 执行最近计划的前 N 条 pending 期次（scheduled_date 升序）。
#[when(expr = "执行该计划前 {int} 期")]
fn execute_first_n_occurrences(world: &mut LedgerWorld, n: usize) {
    let plan_id = world.plan.last_plan_id.clone().expect("尚无定时计划");
    let occ_ids: Vec<String> = {
        let conn = world_conn!(world);
        let mut stmt = conn
            .prepare(
                "SELECT id FROM scheduled_transaction_occurrences \
                 WHERE scheduled_transaction_id=?1 AND status='pending' AND is_deleted=0 \
                 ORDER BY scheduled_date ASC LIMIT ?2",
            )
            .unwrap();
        stmt.query_map(params![plan_id, n as i64], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    for occ_id in occ_ids {
        execute_occurrence_step(world, &occ_id);
    }
}

/// 取消最近的订阅计划（生命周期动词 → update_plan_status 命令体）。
#[when(expr = "取消该订阅计划")]
fn cancel_subscription_plan(world: &mut LedgerWorld) {
    let plan_id = world.plan.last_plan_id.clone().expect("尚无定时计划");
    update_plan_status_verb(world, &plan_id, ScheduledStatus::Cancelled);
}

/// 暂停最近的订阅计划（生命周期动词 → update_plan_status 命令体）。
#[when(expr = "暂停该订阅计划")]
fn pause_subscription_plan(world: &mut LedgerWorld) {
    let plan_id = world.plan.last_plan_id.clone().expect("尚无定时计划");
    update_plan_status_verb(world, &plan_id, ScheduledStatus::Paused);
}

/// 以注入的固定「今日」查询订阅实际花费总览（确定性口径，不依赖真实时钟）。
#[when(expr = "以 {string} 为今日查询订阅花费")]
fn query_spend_with_today(world: &mut LedgerWorld, today: String) {
    let today =
        chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").expect("今日日期应为 YYYY-MM-DD");
    world.plan.last_spend =
        Some(query_subscription_spend(&world_conn!(world), today).expect("查询订阅花费失败"));
}

fn last_spend(world: &LedgerWorld) -> &SubscriptionSpendOverview {
    world.plan.last_spend.as_ref().expect("尚未查询订阅花费")
}

#[then(expr = "本月实际花费应为 {int}")]
fn assert_spend_this_month(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(last_spend(world).this_month_native_cents, expected);
}

#[then(expr = "本年实际花费应为 {int}")]
fn assert_spend_this_year(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(last_spend(world).this_year_native_cents, expected);
}

#[then(expr = "折算月成本应为 {int}")]
fn assert_projected_month(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(last_spend(world).projected_month_native_cents, expected);
}

#[then(expr = "折算年成本应为 {int}")]
fn assert_projected_year(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(last_spend(world).projected_year_native_cents, expected);
}

#[then(expr = "近 12 个月中 {string} 实际花费应为 {int}")]
fn assert_spend_month(world: &mut LedgerWorld, month: String, expected: i64) {
    let overview = last_spend(world);
    let cents = overview
        .months
        .iter()
        .find(|m| m.month == month)
        .unwrap_or_else(|| panic!("12 个月序列应包含 {month}"))
        .native_cents;
    assert_eq!(cents, expected, "{month} 实际花费不符");
}

#[then(expr = "订阅花费行数应为 {int}")]
fn assert_spend_row_count(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(last_spend(world).rows.len(), expected);
}

#[then(expr = "订阅行 {string} 状态应为 {string}")]
fn assert_spend_row_status(world: &mut LedgerWorld, note: String, status: String) {
    let row = last_spend(world)
        .rows
        .iter()
        .find(|r| r.note.as_deref() == Some(note.as_str()))
        .unwrap_or_else(|| panic!("订阅花费行应包含备注 {note}"));
    assert_eq!(row.status, status);
}

#[then(expr = "订阅行 {string} 本年实际花费应为 {int}")]
fn assert_spend_row_year(world: &mut LedgerWorld, note: String, expected: i64) {
    let row = last_spend(world)
        .rows
        .iter()
        .find(|r| r.note.as_deref() == Some(note.as_str()))
        .unwrap_or_else(|| panic!("订阅花费行应包含备注 {note}"));
    assert_eq!(row.this_year_native_cents, expected);
}
