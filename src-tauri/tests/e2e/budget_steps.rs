use chrono::{Datelike, Months, NaiveDate};
use cucumber::{given, then, when};
use rusqlite::params;

use ledger_budget::BudgetInput;
use ledger_budget::BudgetPeriod;
use ledger_budget::{budget_progress_rows, create_budget, delete_budget, update_budget};
use ledger_categories::{
    CategoryInput, create_category, delete_category as delete_category_domain,
    list_categories as list_categories_domain,
};
use ledger_transaction::TransactionInput;
use ledger_transaction::create_transaction_internal;

use crate::common::assert_last_error_contains;
use crate::step_inputs::{expense_input as expense_input_factory, refund_input};
use crate::world::LedgerWorld;

fn scenario_today(world: &mut LedgerWorld) -> NaiveDate {
    *world
        .frozen_today
        .get_or_insert_with(|| chrono::Local::now().date_naive())
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn category_id(conn: &rusqlite::Connection, name: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE name=?1 AND kind='expense' AND is_deleted=0",
        params![name],
        |r| r.get(0),
    )
    .unwrap_or_else(|e| panic!("支出分类 '{}' 不存在: {e}", name))
}

fn category_id_any(conn: &rusqlite::Connection, name: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE name=?1 AND is_deleted=0",
        params![name],
        |r| r.get(0),
    )
    .unwrap_or_else(|e| panic!("分类 '{}' 不存在: {e}", name))
}

fn create_category_via_entry(conn: &rusqlite::Connection, name: &str, parent_name: Option<&str>) {
    let parent_id = parent_name.map(|p| category_id(conn, p));
    create_category(
        conn,
        CategoryInput {
            name: name.into(),
            kind: "expense".into(),
            parent_id,
            icon: None,
        },
    )
    .unwrap();
}

fn create_budget_via_entry(
    conn: &rusqlite::Connection,
    category_id: &str,
    period: &str,
    amount_cents: i64,
    start_date: &str,
) {
    create_budget(
        conn,
        &BudgetInput {
            category_id: category_id.into(),
            period: Some(
                period
                    .parse()
                    .unwrap_or_else(|e| panic!("非法预算周期 '{period}': {e:?}")),
            ),
            amount_cents,
            start_date: start_date.into(),
        },
    )
    .unwrap();
}

fn create_transaction(world: &mut LedgerWorld, input: TransactionInput) -> String {
    let result = create_transaction_internal(&world_conn!(world), input);
    let write = result.unwrap_or_else(|e| panic!("创建交易失败: {e:?}"));
    world.txn.last_transaction_id = Some(write.id.clone());
    write.id
}

fn expense_input(
    world: &LedgerWorld,
    account: &str,
    category_name: &str,
    amount: i64,
    date: String,
) -> TransactionInput {
    TransactionInput {
        category_id: Some(category_id(&world_conn!(world), category_name)),
        ..expense_input_factory(amount, &world.account_id(account), &date)
    }
}

#[given(expr = "存在支出分类 {string}")]
fn create_expense_category(world: &mut LedgerWorld, name: String) {
    create_category_via_entry(&world_conn!(world), &name, None);
}

#[given(expr = "存在支出分类 {string} 属于 {string}")]
fn create_subcategory(world: &mut LedgerWorld, name: String, parent: String) {
    create_category_via_entry(&world_conn!(world), &name, Some(&parent));
}

#[given(expr = "存在收入分类 {string}")]
fn create_income_category(world: &mut LedgerWorld, name: String) {
    create_category(
        &world_conn!(world),
        CategoryInput {
            name,
            kind: "income".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
}

#[given(expr = "为分类 {string} 创建月预算 金额 {int}")]
fn create_monthly_budget(world: &mut LedgerWorld, name: String, amount: i64) {
    let today = scenario_today(world);
    let id = category_id(&world_conn!(world), &name);
    create_budget_via_entry(&world_conn!(world), &id, "monthly", amount, &ymd(today));
}

#[given(expr = "为分类 {string} 创建年预算 金额 {int}")]
fn create_yearly_budget(world: &mut LedgerWorld, name: String, amount: i64) {
    let today = scenario_today(world);
    let id = category_id(&world_conn!(world), &name);
    create_budget_via_entry(&world_conn!(world), &id, "yearly", amount, &ymd(today));
}

#[given(expr = "存量预算 分类 {string} 周期 {string} 金额 {int} 开始日期 {string}")]
fn create_legacy_budget(
    world: &mut LedgerWorld,
    name: String,
    period: String,
    amount: i64,
    start_date: String,
) {
    assert!(
        period == "monthly" || period == "yearly",
        "非法预算周期: {period}"
    );
    let id = category_id(&world_conn!(world), &name);
    create_budget_via_entry(&world_conn!(world), &id, &period, amount, &start_date);
}

#[given(expr = "分类 {string} 本月有一笔支出 {int} 到账户 {string}")]
fn expense_this_month(world: &mut LedgerWorld, name: String, amount: i64, account: String) {
    let today = scenario_today(world);
    let input = expense_input(world, &account, &name, amount, ymd(today));
    create_transaction(world, input);
}

#[given(expr = "分类 {string} 上月有一笔支出 {int} 到账户 {string}")]
fn expense_last_month(world: &mut LedgerWorld, name: String, amount: i64, account: String) {
    let date = ymd(scenario_today(world) - Months::new(1));
    let input = expense_input(world, &account, &name, amount, date);
    create_transaction(world, input);
}

#[given(expr = "分类 {string} 今年一月有一笔支出 {int} 到账户 {string}")]
fn expense_january(world: &mut LedgerWorld, name: String, amount: i64, account: String) {
    let date = ymd(NaiveDate::from_ymd_opt(scenario_today(world).year(), 1, 15).unwrap());
    let input = expense_input(world, &account, &name, amount, date);
    create_transaction(world, input);
}

#[given(expr = "分类 {string} 去年有一笔支出 {int} 到账户 {string}")]
fn expense_last_year(world: &mut LedgerWorld, name: String, amount: i64, account: String) {
    let date = ymd(scenario_today(world) - Months::new(12));
    let input = expense_input(world, &account, &name, amount, date);
    create_transaction(world, input);
}

#[when(expr = "通过预算命令为分类 {string} 创建 {string} 预算 金额 {int}")]
fn create_budget_via_command(world: &mut LedgerWorld, name: String, period: String, amount: i64) {
    let category = category_id_any(&world_conn!(world), &name);
    let input = BudgetInput {
        category_id: category,
        period: Some(
            period
                .parse()
                .unwrap_or_else(|e| panic!("非法预算周期 '{period}': {e:?}")),
        ),
        amount_cents: amount,
        start_date: ymd(scenario_today(world)),
    };
    world.last_error = match world_write!(world, |conn| create_budget(conn, &input)) {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}

#[when(expr = "通过预算命令编辑分类 {string} 的预算金额为 {int}")]
fn update_budget_via_command(world: &mut LedgerWorld, name: String, amount: i64) {
    let cat_id = category_id_any(&world_conn!(world), &name);
    let budget_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM budgets WHERE category_id=?1 AND is_deleted=0 LIMIT 1",
            params![cat_id],
            |r| r.get(0),
        )
        .unwrap_or_else(|e| panic!("分类 '{}' 没有可编辑的预算: {e}", name));
    world.last_error =
        match world_write!(world, |conn| update_budget(conn, &budget_id, amount, None)) {
            Ok(_) => None,
            Err(e) => Some(e.to_string()),
        };
}

#[when(expr = "通过预算命令编辑分类 {string} 的 {string} 预算周期为 {string} 金额为 {int}")]
fn update_budget_period_via_command(
    world: &mut LedgerWorld,
    name: String,
    from_period: String,
    to_period: String,
    amount: i64,
) {
    let cat_id = category_id_any(&world_conn!(world), &name);
    let budget_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM budgets WHERE category_id=?1 AND period=?2 AND is_deleted=0 LIMIT 1",
            params![cat_id, from_period],
            |r| r.get(0),
        )
        .unwrap_or_else(|e| panic!("分类 '{}' 没有周期为 '{}' 的预算: {e}", name, from_period));
    let period: BudgetPeriod = to_period
        .parse()
        .unwrap_or_else(|e| panic!("未知目标周期 '{}': {e}", to_period));
    world.last_error = match world_write!(world, |conn| update_budget(
        conn,
        &budget_id,
        amount,
        Some(period)
    )) {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}

#[when(expr = "删除分类 {string} 的预算")]
fn delete_budget_via_command(world: &mut LedgerWorld, name: String) {
    let cat_id = category_id_any(&world_conn!(world), &name);
    let budget_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM budgets WHERE category_id=?1 AND is_deleted=0 LIMIT 1",
            params![cat_id],
            |r| r.get(0),
        )
        .unwrap_or_else(|e| panic!("分类 '{name}' 没有可删除的预算: {e}"));
    world.last_error = match world_write!(world, |conn| delete_budget(conn, &budget_id)) {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}

#[when(expr = "尝试删除分类 {string}")]
fn delete_category_via_command(world: &mut LedgerWorld, name: String) {
    let id = category_id_any(&world_conn!(world), &name);
    world.last_error = match world_write!(world, |conn| delete_category_domain(conn, &id)) {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}

#[then(expr = "删除应成功")]
fn assert_delete_category_succeeded(world: &mut LedgerWorld) {
    assert!(
        world.last_error.is_none(),
        "预期删除成功，实际错误: {:?}",
        world.last_error
    );
}

#[then(expr = "删除应失败并提示 {string}")]
fn assert_delete_category_failed(world: &mut LedgerWorld, needle: String) {
    assert_last_error_contains(world, &needle);
}

#[then(expr = "分类 {string} 仍应存在")]
fn assert_category_still_exists(world: &mut LedgerWorld, name: String) {
    let cats = list_categories_domain(&world_conn!(world), false).unwrap();
    assert!(
        cats.iter().any(|c| c.name == name),
        "分类 '{}' 应仍存在于读回结果中",
        name
    );
}

#[then(expr = "分类 {string} 不应存在")]
fn assert_category_gone(world: &mut LedgerWorld, name: String) {
    let cats = list_categories_domain(&world_conn!(world), false).unwrap();
    assert!(
        !cats.iter().any(|c| c.name == name),
        "分类 '{}' 不应再出现在读回结果中",
        name
    );
}

#[when(expr = "查询预算进度")]
fn query_budget_progress(world: &mut LedgerWorld) {
    let today = scenario_today(world);
    world.report.last_budget_progress = budget_progress_rows(&world_conn!(world), today).unwrap();
}

#[when(expr = "上一笔支出本月收到退款 {int}")]
fn refund_last_expense(world: &mut LedgerWorld, amount: i64) {
    let expense_id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("场景中没有可退款的前序支出");
    let input = refund_input(
        amount,
        &world.account_id("现金"),
        &expense_id,
        &ymd(scenario_today(world)),
    );
    create_transaction(world, input);
}

#[then(expr = "分类 {string} 的预算进度应为 {int}")]
fn assert_budget_spent(world: &mut LedgerWorld, name: String, expected: i64) {
    let row = world
        .report
        .last_budget_progress
        .iter()
        .find(|p| p.category_name == name)
        .unwrap_or_else(|| panic!("预算进度中不存在分类 '{}'", name));
    assert_eq!(
        row.spent_cents, expected,
        "分类 '{}' 的预算进度不符（expense_net 口径）",
        name
    );
}

#[then(expr = "分类 {string} 的预算应超支")]
fn assert_over_budget(world: &mut LedgerWorld, name: String) {
    let row = world
        .report
        .last_budget_progress
        .iter()
        .find(|p| p.category_name == name)
        .unwrap_or_else(|| panic!("预算进度中不存在分类 '{}'", name));
    assert!(row.over_budget, "分类 '{}' 应已超支", name);
}

#[then(expr = "分类 {string} 的预算不应超支")]
fn assert_not_over_budget(world: &mut LedgerWorld, name: String) {
    let row = world
        .report
        .last_budget_progress
        .iter()
        .find(|p| p.category_name == name)
        .unwrap_or_else(|| panic!("预算进度中不存在分类 '{}'", name));
    assert!(!row.over_budget, "分类 '{}' 不应超支", name);
}

#[then(expr = "创建应失败并提示 {string}")]
fn assert_create_budget_failed(world: &mut LedgerWorld, needle: String) {
    assert_last_error_contains(world, &needle);
}

#[then(expr = "创建应成功")]
fn assert_create_budget_succeeded(world: &mut LedgerWorld) {
    assert!(
        world.last_error.is_none(),
        "预期创建成功，实际错误: {:?}",
        world.last_error
    );
}

#[then(expr = "编辑预算应失败并提示 {string}")]
fn assert_update_budget_failed(world: &mut LedgerWorld, needle: String) {
    assert_last_error_contains(world, &needle);
}

#[then(expr = "编辑预算应成功")]
fn assert_update_budget_succeeded(world: &mut LedgerWorld) {
    assert!(
        world.last_error.is_none(),
        "预期编辑成功，实际错误: {:?}",
        world.last_error
    );
}

#[then(expr = "分类 {string} 的预算金额应为 {int}")]
fn assert_budget_amount(world: &mut LedgerWorld, name: String, expected: i64) {
    let row = world
        .report
        .last_budget_progress
        .iter()
        .find(|p| p.category_name == name)
        .unwrap_or_else(|| panic!("预算进度中不存在分类 '{}'", name));
    assert_eq!(
        row.budget.amount_cents, expected,
        "分类 '{}' 的预算金额不符",
        name
    );
}

#[then(expr = "分类 {string} 的预算周期应为 {string}")]
fn assert_budget_period(world: &mut LedgerWorld, name: String, expected: String) {
    let row = world
        .report
        .last_budget_progress
        .iter()
        .find(|p| p.category_name == name)
        .unwrap_or_else(|| panic!("预算进度中不存在分类 '{}'", name));
    assert_eq!(
        row.budget.period.to_string(),
        expected,
        "分类 '{}' 的预算周期不符",
        name
    );
}

#[then(expr = "分类 {string} 的预算行数应为 {int}")]
fn assert_budget_row_count(world: &mut LedgerWorld, name: String, expected: i64) {
    let id = category_id_any(&world_conn!(world), &name);
    let count: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM budgets WHERE category_id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, expected, "分类 '{}' 的预算行数不符", name);
}

#[then(expr = "分类 {string} 的预算金额仍应为 {int}")]
fn assert_budget_amount_unchanged(world: &mut LedgerWorld, name: String, expected: i64) {
    let id = category_id_any(&world_conn!(world), &name);
    let conn = world_conn!(world);
    let mut stmt = conn
        .prepare("SELECT amount_cents FROM budgets WHERE category_id=?1 AND is_deleted=0")
        .unwrap();
    let amounts: Vec<i64> = stmt
        .query_map(params![id], |r| r.get(0))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(
        amounts,
        vec![expected],
        "分类 '{}' 的原预算金额应保持不变",
        name
    );
}
