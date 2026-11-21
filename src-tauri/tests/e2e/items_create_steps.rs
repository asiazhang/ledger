use cucumber::{then, when};

use ledger_infra::error::AppError;
use ledger_item::cost;
use ledger_item::domain;
use ledger_item::{ItemInput, ItemStatus};
use ledger_transaction::{TransactionInput, create_transaction_internal};

use crate::common::assert_last_error_contains;
use crate::items_common::build_input;
use crate::step_inputs::expense_input;
use crate::step_verbs::create_account_verb;
use crate::world::LedgerWorld;

fn scaffold_purchase_tx(
    world: &mut LedgerWorld,
    date: &str,
    cost_cents: i64,
    currency: &str,
) -> String {
    let account_name = format!("物品脚手架({currency})");
    let account_id = match world.account_name_to_id.get(&account_name) {
        Some(id) => id.clone(),
        None => create_account_verb(world, &account_name, "cash", currency, None),
    };
    let input = TransactionInput {
        currency_code: currency.into(),
        ..expense_input(cost_cents, &account_id, date)
    };
    create_transaction_internal(&world_conn!(world), input)
        .unwrap_or_else(|e| panic!("脚手架购买交易应创建成功但失败: {e}"))
        .id
}

#[when(expr = "创建物品 {string} 购买日期 {string} 总成本 {int} 币种 {string}")]
fn create_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
) {
    let tx_id = scaffold_purchase_tx(world, &date, cost_cents, &currency);
    let input = ItemInput {
        purchase_transaction_id: Some(tx_id),
        ..build_input(&name, date, cost_cents, &currency)
    };
    let mut signals = 0;
    let result = domain::create_item(&world_conn!(world), input, &mut || signals += 1);
    match result {
        Ok(id) => {
            world.item.last_item_id = Some(id);
            world.item.item_signal_count = signals;
        }
        Err(AppError::Invalid(msg)) => panic!("创建物品应成功但失败: {msg}"),
        Err(e) => panic!("创建物品应成功但失败: {e}"),
    }
}

#[when(expr = "创建物品 {string} 今天购买 总成本 {int} 币种 {string}")]
fn create_item_bought_today(
    world: &mut LedgerWorld,
    name: String,
    cost_cents: i64,
    currency: String,
) {
    let date = cost::today().format("%Y-%m-%d").to_string();
    create_item(world, name, date, cost_cents, currency);
}

#[when(expr = "创建物品 {string} 今天前 {int} 天购买 总成本 {int} 币种 {string}")]
fn create_item_bought_days_ago(
    world: &mut LedgerWorld,
    name: String,
    days_ago: i64,
    cost_cents: i64,
    currency: String,
) {
    let date = (cost::today() - chrono::Duration::days(days_ago))
        .format("%Y-%m-%d")
        .to_string();
    create_item(world, name, date, cost_cents, currency);
}

#[when(expr = "尝试创建物品 {string} 购买日期 {string} 总成本 {int} 币种 {string}")]
fn try_create_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
) {
    let tx_id = scaffold_purchase_tx(world, &date, cost_cents, &currency);
    let input = ItemInput {
        purchase_transaction_id: Some(tx_id),
        ..build_input(&name, date, cost_cents, &currency)
    };
    let mut signals = 0;
    let result = domain::create_item(&world_conn!(world), input, &mut || signals += 1);
    world.item.item_signal_count = signals;
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "物品列表应包含 {int} 件物品")]
fn refresh_and_check_item_count(world: &mut LedgerWorld, expected: usize) {
    world.item.items_list = domain::list_items(&world_conn!(world)).expect("列出物品失败");
    assert_eq!(
        world.item.items_list.len(),
        expected,
        "物品件数不匹配: {:?}",
        world
            .item
            .items_list
            .iter()
            .map(|i| &i.item.name)
            .collect::<Vec<_>>()
    );
}

#[then(expr = "第 {int} 件物品名称应为 {string}")]
fn check_item_name(world: &mut LedgerWorld, n: usize, name: String) {
    assert_eq!(world.item.nth(n).item.name, name);
}

#[then(expr = "第 {int} 件物品总成本应为 {int} 币种应为 {string} 本位币成本应为 {int}")]
fn check_item_amounts(
    world: &mut LedgerWorld,
    n: usize,
    cost_cents: i64,
    currency: String,
    native_cents: i64,
) {
    let item = &world.item.nth(n).item;
    assert_eq!(item.total_cost_cents, cost_cents);
    assert_eq!(item.currency_code, currency);
    assert_eq!(item.cost_native_cents, native_cents);
}

#[then(expr = "第 {int} 件物品状态应为 {string}")]
fn check_item_status(world: &mut LedgerWorld, n: usize, status: String) {
    let parsed = ItemStatus::parse(&status).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(world.item.nth(n).item.status, parsed);
}

#[then(expr = "第 {int} 件物品已用天数应为 {int} 每天成本应为 {float}")]
fn check_item_daily_cost(world: &mut LedgerWorld, n: usize, days: i64, per_day: f64) {
    let entry = world.item.nth(n);
    assert_eq!(entry.used_days, days);
    assert!(
        (entry.per_day_cents - per_day).abs() < 1e-6,
        "每天成本不匹配: 期望 {per_day}, 实际 {}",
        entry.per_day_cents
    );
}

#[then(expr = "第 {int} 件物品应有唯一 ID 与审计字段")]
fn check_item_audit_fields(world: &mut LedgerWorld, n: usize) {
    let item = &world.item.nth(n).item;
    assert!(!item.id.is_empty(), "物品 id 不应为空");
    assert_eq!(item.version, 1, "新物品 version 应为 1");
    assert!(!item.device_id.is_empty(), "device_id 不应为空");
    assert!(!item.created_at.is_empty(), "created_at 不应为空");
    assert!(!item.updated_at.is_empty(), "updated_at 不应为空");
    assert!(!item.is_deleted);
}

#[then(expr = "写入后应发出 {int} 次失效信号")]
fn check_item_signals(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.item.item_signal_count, expected,
        "失效信号次数不匹配（生产路径对应 ledger:changed）"
    );
}

#[then(expr = "未发出失效信号")]
fn check_no_item_signals(world: &mut LedgerWorld) {
    assert_eq!(world.item.item_signal_count, 0, "不应发出失效信号");
}

#[then(expr = "物品创建应返回错误 {string}")]
fn check_item_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}
