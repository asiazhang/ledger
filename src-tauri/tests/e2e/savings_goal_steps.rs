//! 储蓄目标跨模块用户旅程 BDD 步骤（spec #1750 / issue #1757，e2e 收尾票）：
//! 建目标 → 自动建户 → 记转账蓄水 → 进度上涨 → 达成提示 → 归档收起，一条旅程
//! 走通核心交易域与储蓄目标域的接缝。
//!
//! 造数走既有步骤动词与公开写入口（CONTEXT-testing「步骤动词」「公开写入口
//! （测试侧）」）：目标写入经储蓄目标域公开编排入口（`create_savings_goal` /
//! `archive_savings_goal`，域内自持事务同形 IPC 命令体——`ensure_transaction`
//! 嵌套感知保证在 `world_write!` 外直调也原子提交），
//! 蓄水走既有转账动词（`create_transfer`，真实 transfer 流水），账户可读性 /
//! 类型经既有账户查询断言；业务不变量（余额缓存、绑定、达成派生）由产品代码
//! 保证，不直插 SQL。断言对准用户可观察结果（进度数字、达成态、列表分组），
//! 不对准实现形状（ADR-0087）。
//!
//! 读命令（`list_savings_goal_progress`）快照入报表组（`world.report`
//! `.last_savings_goal_progress`——「进行度查询」类聚合消费端，`last_budget_progress`
//! 同款先例），快照分组零新增组（测试基础设施域「快照分组」闭集纪律）。

use cucumber::{then, when};

use ledger_savings_goal::{
    SavingsGoalStatus, archive_savings_goal, create_savings_goal, list_savings_goal_progress,
};

use crate::common::query_accounts_by_name;
use crate::step_verbs;
use crate::world::LedgerWorld;

/// 读一次蓄水进度快照（命令层同款注入口径 = 本地今日；读闭包收进同一读事务）。
/// 场景内多次 When 写入后显式查询，Then 侧只消费快照（与预算进度同款断言形态）。
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

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

/// 创建储蓄目标（域公开编排入口，`ensure_transaction` 嵌套感知：经 `world_write!`
/// 包裹与 IPC 命令体同形走统一写入口）。自动建户由域内同一事务保证——本步骤
/// 只拿返回的目标 id，断言侧经进度读数的绑定与账户查询可读。
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
            // 目标名权威、专属账户名随动（创建即同名）：注册名称→id 供既有
            // 「账户余额」断言与转账动词按名解析——自动建户是旅程的可观察结果。
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

/// 归档目标：状态翻转经域公开生命周期入口（词汇表「达成与归档」）。
#[when(expr = "归档目标 {string}")]
fn archive_goal(world: &mut LedgerWorld, name: String) {
    let id = goal_id_by_name(world, &name);
    archive_savings_goal(&world_conn!(world), &id).expect("归档目标失败");
}

/// 查询蓄水进度快照（读命令内核同款注入口径）。
#[when(expr = "查询目标进度")]
fn query_progress(world: &mut LedgerWorld) {
    refresh_progress(world);
}

/// 挂一条定时转账计划给目标专属账户（进度节奏来源「计划」的旅程断言用）：
/// 经既有定时转账计划动词创建（无限循环形态——月存节奏的语义本体）。
#[when(expr = "挂定时转账计划 每月 {int} 从 {string} 到 {string} 起始日期 {string}")]
fn add_plan(world: &mut LedgerWorld, amount: i64, from: String, to: String, start: String) {
    step_verbs::create_scheduled_transfer_plan(world, amount, &from, &to, None, &start);
}

/// 按目标名取目标 id（名称解析 → 域公开生命周期入口 id 形参；目标行按名唯一——
/// 目标名权威且专属账户名随动，场景内目标名不重复）。
fn goal_id_by_name(world: &LedgerWorld, name: &str) -> String {
    let conn = world_conn!(world);
    conn.query_row(
        "SELECT id FROM goals WHERE name=?1 AND is_deleted=0",
        [name],
        |r| r.get(0),
    )
    .expect("目标不存在，先铺垫创建步骤")
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

/// 蓄水进度断言（读数联动 AC）：已存 = 专属账户余额（余额缓存口径）、还差 =
/// 带符号差值、达成 = 读时派生纯展示态——转账后进度上涨即由本断言承载。
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

/// 自动建户接线证明：目标绑定的专属账户在用户侧账户列表可见且为 other 类型
///（ADR-0133 决策 2）。
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

/// 查询目标进度的读命令显式步骤（Then 形态别名，场景编排用）：cucumber 的
/// And 继承上一句步骤类型（When 之后的 And 按 When 匹配），Then 引导的续断
/// 需要同名 Then 步骤——两形态同体，读命令单点 [`refresh_progress`]。
#[then(expr = "查询目标进度")]
fn query_progress_then(world: &mut LedgerWorld) {
    refresh_progress(world);
}

/// 已达成目标的关联计划提示（词汇表「达成与归档」：达成不自动暂停关联计划，
/// 计划仍执行时目标页给提示）：断言达成态在场 + 节奏来源为计划（计划仍在执行
/// 的读命令可见面）——前端 hint 渲染断言归组件测试，此处断言其后端读数前提。
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

/// 默认列表（进行中）分组断言：status='active' 的目标名集合（视图层按 status
/// 分组展示，读命令只带 status——分组即用户可观察列表）。
#[then(expr = "默认列表（进行中）{word} {string}")]
fn assert_active_list(world: &mut LedgerWorld, op: String, name: String) {
    // Then 侧只消费快照（「查询目标进度」已读；场景读数在动作后显式刷新）。
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

/// 归档列表分组断言 + 进度读数原样（归档不删任何东西，词汇表「达成与归档」）。
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
