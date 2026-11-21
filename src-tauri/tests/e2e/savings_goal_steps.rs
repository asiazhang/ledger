use cucumber::{then, when};

use ledger_savings_goal::{
    SavingsGoalStatus, archive_savings_goal, create_savings_goal, list_savings_goal_progress,
};

use crate::common::query_accounts_by_name;
use crate::step_verbs;
use crate::world::LedgerWorld;

fn refresh_progress(world: &mut LedgerWorld) {
    let today = *world
        .frozen_today
        .get_or_insert_with(|| chrono::Local::now().date_naive());
    world.report.last_savings_goal_progress =
        Some(list_savings_goal_progress(&world_conn!(world), today).expect("蓄水进度读取失败"));
}

fn progress_row(world: &LedgerWorld, name: &str) -> ledger_savings_goal::SavingsGoalProgress {
    world
        .report
        .last_savings_goal_progress
        .as_ref()
        .expect("场景应先查询目标进度")
        .iter()
        .find(|p| p.goal.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("目标进度读数中不存在目标 '{name}'"))
}

#[when(expr = "创建储蓄目标 {string} 目标额 {int} 截止日 {string}")]
fn create_goal(world: &mut LedgerWorld, name: String, target: i64, deadline: String) {
    let result = world_write!(world, |conn| create_savings_goal(
        conn,
        &ledger_savings_goal::SavingsGoalInput {
            name: name.clone(),
            target_amount_cents: target,
            deadline: Some(deadline),
        },
    ));
    match result {
        Ok(id) => {
            world.last_error = None;
            let account_id: String = world_conn!(world)
                .query_row("SELECT account_id FROM goals WHERE id=?1", [&id], |r| {
                    r.get(0)
                })
                .expect("目标绑定的专属账户应存在");
            world.account_name_to_id.insert(name.clone(), account_id);
        }
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(expr = "归档目标 {string}")]
fn archive_goal(world: &mut LedgerWorld, name: String) {
    let id = goal_id_by_name(world, &name);
    archive_savings_goal(&world_conn!(world), &id).expect("归档目标失败");
}

#[when(expr = "查询目标进度")]
fn query_progress(world: &mut LedgerWorld) {
    refresh_progress(world);
}

#[when(expr = "挂定时转账计划 每月 {int} 从 {string} 到 {string} 起始日期 {string}")]
fn add_plan(world: &mut LedgerWorld, amount: i64, from: String, to: String, start: String) {
    step_verbs::create_scheduled_transfer_plan(world, amount, &from, &to, None, &start);
}

fn goal_id_by_name(world: &LedgerWorld, name: &str) -> String {
    let conn = world_conn!(world);
    conn.query_row(
        "SELECT id FROM goals WHERE name=?1 AND is_deleted=0",
        [name],
        |r| r.get(0),
    )
    .expect("目标不存在，先铺垫创建步骤")
}

#[then(expr = "目标进度读数 {string} 已存 {int} 还差 {int} {word}")]
fn assert_progress(
    world: &mut LedgerWorld,
    name: String,
    saved: i64,
    remaining: i64,
    achieved_word: String,
) {
    let achieved = match achieved_word.as_str() {
        "已达成" => true,
        "未达成" => false,
        other => panic!("达成语仅支持 已达成/未达成，实际 '{other}'"),
    };
    let row = progress_row(world, &name);
    assert_eq!(
        row.saved_cents, saved,
        "目标 '{}' 已存不符（余额缓存口径）",
        name
    );
    assert_eq!(row.remaining_cents, remaining, "目标 '{}' 还差不符", name);
    assert_eq!(
        row.achieved, achieved,
        "目标 '{}' 达成态不符（余额 ≥ 目标额的读时派生）",
        name
    );
}

#[then(expr = "专属账户 {string} 经账户域可读且类型为 other")]
fn assert_bound_account(world: &mut LedgerWorld, name: String) {
    let names = query_accounts_by_name(&world_conn!(world));
    assert!(
        names.contains(&name),
        "目标专属账户 '{name}' 应经账户域可读，实际 {:?}",
        names
    );
    let kind: String = world_conn!(world)
        .query_row(
            "SELECT type FROM accounts WHERE name=?1 AND is_deleted=0",
            [&name],
            |r| r.get(0),
        )
        .expect("账户不存在");
    let kind = kind
        .parse::<ledger_accounts::AccountType>()
        .expect("账户类型应为合法闭集值");
    assert_eq!(
        kind,
        ledger_accounts::AccountType::Other,
        "目标专属账户 '{name}' 应为 other 类型（ADR-0133 决策 2）"
    );
}

#[then(expr = "查询目标进度")]
fn query_progress_then(world: &mut LedgerWorld) {
    refresh_progress(world);
}

#[then(expr = "已达成目标 {string} 的关联计划提示在场（节奏来源为计划）")]
fn assert_achieved_plan_hint(world: &mut LedgerWorld, name: String) {
    let row = progress_row(world, &name);
    assert!(row.achieved, "目标 '{name}' 应为达成态（余额 ≥ 目标额）");
    assert_eq!(
        row.pace_source.as_ref(),
        Some(&ledger_savings_goal::SavingsGoalPaceSource::Plan),
        "目标 '{name}' 的节奏来源应为关联计划（提示在场的前提）"
    );
}

#[then(expr = "默认列表（进行中）{word} {string}")]
fn assert_active_list(world: &mut LedgerWorld, op: String, name: String) {
    let names: Vec<String> = world
        .report
        .last_savings_goal_progress
        .as_ref()
        .expect("进度快照应已读出")
        .iter()
        .filter(|p| p.goal.status == SavingsGoalStatus::Active)
        .map(|p| p.goal.name.clone())
        .collect();
    match op.as_str() {
        "含" => assert!(
            names.contains(&name),
            "进行中列表应包含 '{name}'，实际 {names:?}"
        ),
        "不含" => assert!(
            !names.contains(&name),
            "进行中列表不应包含 '{name}'，实际 {names:?}"
        ),
        other => panic!("列表断言词仅支持 含/不含，实际 '{other}'"),
    }
}

#[then(expr = "归档列表含 {string} 且进度读数原样（已存 {int}）")]
fn assert_archived_list(world: &mut LedgerWorld, name: String, saved: i64) {
    let row = progress_row(world, &name);
    assert_eq!(
        row.goal.status,
        SavingsGoalStatus::Archived,
        "目标 '{name}' 应为归档态"
    );
    assert_eq!(
        row.saved_cents, saved,
        "归档目标 '{name}' 进度读数应原样保留"
    );
}
