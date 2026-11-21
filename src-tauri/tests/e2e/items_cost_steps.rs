use cucumber::{then, when};

use ledger_infra::error::AppError;
use ledger_item::ItemDailyCost;
use ledger_item::cost;
use ledger_item::domain::calculate_item_cost;

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

fn calc_item_cost(
    world: &mut LedgerWorld,
    id: &str,
    reference_date: Option<String>,
) -> Result<ItemDailyCost, AppError> {
    calculate_item_cost(&world_conn!(world), id, reference_date.as_deref())
}

#[when(expr = "按最近创建的物品计算每天成本 不带参考日")]
fn calc_item_cost_default(world: &mut LedgerWorld) {
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    world.item.last_item_cost = Some(calc_item_cost(world, &id, None).expect("计算每天成本应成功"));
}

#[when(expr = "按最近创建的物品计算每天成本 今天前 {int} 天为参考日")]
fn calc_item_cost_days_ago(world: &mut LedgerWorld, days_ago: i64) {
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    let date = (cost::today() - chrono::Duration::days(days_ago))
        .format("%Y-%m-%d")
        .to_string();
    world.item.last_item_cost =
        Some(calc_item_cost(world, &id, Some(date)).expect("计算每天成本应成功"));
}

#[when(expr = "按最近创建的物品计算每天成本 今天后 {int} 天为参考日")]
fn calc_item_cost_days_later(world: &mut LedgerWorld, days_later: i64) {
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    let date = (cost::today() + chrono::Duration::days(days_later))
        .format("%Y-%m-%d")
        .to_string();
    world.item.last_item_cost =
        Some(calc_item_cost(world, &id, Some(date)).expect("计算每天成本应成功"));
}

#[when(expr = "按最近创建的物品计算每天成本 参考日 {string}")]
fn calc_item_cost_fixed_ref(world: &mut LedgerWorld, date: String) {
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可计算"));
    world.item.last_item_cost =
        Some(calc_item_cost(world, &id, Some(date)).expect("计算每天成本应成功"));
}

#[when(expr = "尝试按最近创建的物品计算每天成本 参考日 {string}")]
fn try_calc_item_cost(world: &mut LedgerWorld, date: String) {
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| "no-such-item-id".into());
    world.last_error = match calc_item_cost(world, &id, Some(date)) {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

#[when(expr = "尝试按不存在的物品计算每天成本")]
fn try_calc_item_cost_missing(world: &mut LedgerWorld) {
    world.last_error = match calc_item_cost(world, "no-such-item-id", None) {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "计算结果已用天数应为 {int} 分子应为 {int} 每天成本应为 {float}")]
fn check_calc_item_cost(world: &mut LedgerWorld, days: i64, numerator: i64, per_day: f64) {
    let result = world
        .item
        .last_item_cost
        .as_ref()
        .unwrap_or_else(|| panic!("没有计算结果（先调「按最近创建的物品计算每天成本」）"));
    assert_eq!(result.used_days, days, "重算天数不匹配");
    assert_eq!(result.numerator_cents, numerator, "重算分子不匹配");
    assert!(
        (result.per_day_cents - per_day).abs() < 1e-6,
        "重算每天成本不匹配: 期望 {per_day}, 实际 {}",
        result.per_day_cents
    );
}

#[then(expr = "计算每天成本应返回错误 {string}")]
fn check_calc_item_cost_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}
