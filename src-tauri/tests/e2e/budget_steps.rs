//! 预算进度滚动窗口 e2e 步骤定义（issue #182）。
//!
//! 窗口随自然时间滚动：步骤里的「本月 / 上月 / 今年一月 / 去年」
//! 均以本地今日（`chrono::Local::now`，与命令层注入口径一致）动态推算，
//! 场景在任何日期运行都成立。每个场景开始时冻结一次 today
//! （[`scenario_today`]），避免跨自然月/年边界午夜运行时各步骤口径漂移。
//! 预算行与支出分类为夹具（直接插库），交易走与真实写路径一致的行为层
//! （`create_transaction_internal`），进度经核心函数 `budget_progress_rows`（命令层同款注入）查询。

use chrono::{Datelike, Months, NaiveDate};
use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::budget::{budget_progress_rows, create_budget, delete_budget, update_budget};
use tauri_app_lib::commands::categories::{delete_category_internal, list_categories_internal};
use tauri_app_lib::commands::transactions::create_transaction_internal;
use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::models::{BudgetInput, TransactionInput};
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// 夹具工具
// ---------------------------------------------------------------------------

/// 本地今日（与命令层 `budget_progress` 注入口径一致）。
/// 每个场景首次调用时冻结，之后整个场景复用同一 today。
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

/// 不限 kind 的分类查找（拒绝路径场景要拿收入分类的 id）。
fn category_id_any(conn: &rusqlite::Connection, name: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE name=?1 AND is_deleted=0",
        params![name],
        |r| r.get(0),
    )
    .unwrap_or_else(|e| panic!("分类 '{}' 不存在: {e}", name))
}

fn insert_category(conn: &rusqlite::Connection, name: &str, parent_name: Option<&str>) {
    let parent_id = parent_name.map(|p| category_id(conn, p));
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'expense',?3,?4,?4,1,?5)",
        params![new_uuid(), name, parent_id, now, device_id()],
    )
    .unwrap();
}

fn insert_budget_row(
    conn: &rusqlite::Connection,
    category_id: &str,
    period: &str,
    amount_cents: i64,
    start_date: &str,
) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?6,1,?7,0)",
        params![new_uuid(), category_id, period, amount_cents, start_date, now, device_id()],
    )
    .unwrap();
}

/// 经行为层落一笔带分类的支出/退款类交易，返回交易 id。
fn create_transaction(world: &mut LedgerWorld, input: TransactionInput) -> String {
    let result = create_transaction_internal(&world_conn!(world), input);
    let write = result.unwrap_or_else(|e| panic!("创建交易失败: {e:?}"));
    world.last_transaction_id = Some(write.id.clone());
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
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(account),
        to_account_id: None,
        category_id: Some(category_id(&world_conn!(world), category_name)),
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    }
}

// ---------------------------------------------------------------------------
// Given：分类与预算夹具
// ---------------------------------------------------------------------------

#[given(expr = "存在支出分类 {string}")]
fn create_category(world: &mut LedgerWorld, name: String) {
    insert_category(&world_conn!(world), &name, None);
}

#[given(expr = "存在支出分类 {string} 属于 {string}")]
fn create_subcategory(world: &mut LedgerWorld, name: String, parent: String) {
    insert_category(&world_conn!(world), &name, Some(&parent));
}

#[given(expr = "存在收入分类 {string}")]
fn create_income_category(world: &mut LedgerWorld, name: String) {
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO categories (id,name,kind,parent_id,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'income',NULL,?3,?3,1,?4)",
            params![new_uuid(), name, now, device_id()],
        )
        .unwrap();
}

#[given(expr = "为分类 {string} 创建月预算 金额 {int}")]
fn create_monthly_budget(world: &mut LedgerWorld, name: String, amount: i64) {
    let today = scenario_today(world);
    let id = category_id(&world_conn!(world), &name);
    insert_budget_row(&world_conn!(world), &id, "monthly", amount, &ymd(today));
}

#[given(expr = "为分类 {string} 创建年预算 金额 {int}")]
fn create_yearly_budget(world: &mut LedgerWorld, name: String, amount: i64) {
    let today = scenario_today(world);
    let id = category_id(&world_conn!(world), &name);
    insert_budget_row(&world_conn!(world), &id, "yearly", amount, &ymd(today));
}

/// 模拟存量行：带历史开始日期的预算（旧数据零迁移，直接按新规则滚动生效）。
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
    insert_budget_row(&world_conn!(world), &id, &period, amount, &start_date);
}

// ---------------------------------------------------------------------------
// Given：相对当前自然周期落交易
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// When：经真实写路径创建预算（issue #183 拒绝路径）
// ---------------------------------------------------------------------------

/// 经预算命令同形态（连接层统一写入口，ADR-0032 / issue #245）创建预算：
/// 成功清空 last_error，失败记入 last_error 供拒绝路径断言。
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
    world.last_error = match world.db.write(|conn| create_budget(conn, &input)) {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}

// ---------------------------------------------------------------------------
// When：编辑预算金额（issue #184）
// ---------------------------------------------------------------------------

/// 经预算命令同形态（连接层统一写入口，ADR-0032 / issue #245）编辑预算金额：
/// 成功清空 last_error，失败记入 last_error 供拒绝路径断言。
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
    world.last_error = match world
        .db
        .write(|conn| update_budget(conn, &budget_id, amount))
    {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}

/// 经预算命令同形态（连接层统一写入口，ADR-0032 / issue #245）软删除分类的预算：
/// 成功清空 last_error，失败记入 last_error 供断言。
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
    world.last_error = match world.db.write(|conn| delete_budget(conn, &budget_id)) {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}

// ---------------------------------------------------------------------------
// When：删除分类（预算删除守卫，issue #355）
// ---------------------------------------------------------------------------

/// 经分类命令同形态（连接层统一写入口，ADR-0032）删除分类：
/// 成功清空 last_error，失败记入 last_error 供拒绝路径断言（守卫拒绝不 panic，走真实命令路径）。
#[when(expr = "尝试删除分类 {string}")]
fn delete_category_via_command(world: &mut LedgerWorld, name: String) {
    let id = category_id_any(&world_conn!(world), &name);
    world.last_error = match world.db.write(|conn| delete_category_internal(conn, &id)) {
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

/// 命令层可见结果：分类在读回列表中（与真实读路径同款）。
#[then(expr = "分类 {string} 仍应存在")]
fn assert_category_still_exists(world: &mut LedgerWorld, name: String) {
    let cats = list_categories_internal(&world_conn!(world), false).unwrap();
    assert!(
        cats.iter().any(|c| c.name == name),
        "分类 '{}' 应仍存在于读回结果中",
        name
    );
}

#[then(expr = "分类 {string} 不应存在")]
fn assert_category_gone(world: &mut LedgerWorld, name: String) {
    let cats = list_categories_internal(&world_conn!(world), false).unwrap();
    assert!(
        !cats.iter().any(|c| c.name == name),
        "分类 '{}' 不应再出现在读回结果中",
        name
    );
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "查询预算进度")]
fn query_budget_progress(world: &mut LedgerWorld) {
    let today = scenario_today(world);
    world.last_budget_progress = budget_progress_rows(&world_conn!(world), today).unwrap();
}

/// 上一笔支出本月收到退款：走行为层。Writer 归一化会以原支出覆盖账户/币种/分类，
/// 此处传入的账户与币种仅为满足入参形状，实际不生效。
#[when(expr = "上一笔支出本月收到退款 {int}")]
fn refund_last_expense(world: &mut LedgerWorld, amount: i64) {
    let expense_id = world
        .last_transaction_id
        .clone()
        .expect("场景中没有可退款的前序支出");
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Refund,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id("现金"),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: Some(expense_id),
        note: None,
        date: ymd(scenario_today(world)),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    create_transaction(world, input);
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "分类 {string} 的预算进度应为 {int}")]
fn assert_budget_spent(world: &mut LedgerWorld, name: String, expected: i64) {
    let row = world
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
        .last_budget_progress
        .iter()
        .find(|p| p.category_name == name)
        .unwrap_or_else(|| panic!("预算进度中不存在分类 '{}'", name));
    assert!(row.over_budget, "分类 '{}' 应已超支", name);
}

#[then(expr = "分类 {string} 的预算不应超支")]
fn assert_not_over_budget(world: &mut LedgerWorld, name: String) {
    let row = world
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

/// 经进度读路径断言编辑后的金额（保存后列表/进度即时反映新金额）。
#[then(expr = "分类 {string} 的预算金额应为 {int}")]
fn assert_budget_amount(world: &mut LedgerWorld, name: String, expected: i64) {
    let row = world
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
