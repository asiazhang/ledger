use cucumber::{then, when};

use ledger_infra::error::AppError;
use ledger_item::ItemInput;
use ledger_item::cost;
use ledger_item::domain;

use crate::common::assert_last_error_contains;
use crate::items_common::build_input;
use crate::world::LedgerWorld;

fn with_note(mut input: ItemInput, note: &str) -> ItemInput {
    input.note = if note.is_empty() {
        None
    } else {
        Some(note.to_string())
    };
    input
}

#[when(
    expr = "修改物品名称为 {string} 购买日期 {string} 总成本 {int} 币种 {string} 备注为 {string}"
)]
fn update_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let mut signals = 0;
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可修改"));
    let result = domain::update_item(
        &world_conn!(world),
        &id,
        with_note(build_input(&name, date, cost_cents, &currency), &note),
        &mut || signals += 1,
    );
    match result {
        Ok(()) => world.item.item_signal_count = signals,
        Err(e) => panic!("修改物品应成功但失败: {e}"),
    }
}

#[when(
    expr = "修改物品名称为 {string} 今天前 {int} 天购买 总成本 {int} 币种 {string} 备注为 {string}"
)]
fn update_item_days_ago(
    world: &mut LedgerWorld,
    name: String,
    days_ago: i64,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let date = (cost::today() - chrono::Duration::days(days_ago))
        .format("%Y-%m-%d")
        .to_string();
    update_item(world, name, date, cost_cents, currency, note);
}

#[when(
    expr = "尝试修改物品名称为 {string} 购买日期 {string} 总成本 {int} 币种 {string} 备注为 {string}"
)]
fn try_update_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let mut signals = 0;
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| "no-such-item".into());
    let result = domain::update_item(
        &world_conn!(world),
        &id,
        with_note(build_input(&name, date, cost_cents, &currency), &note),
        &mut || signals += 1,
    );
    world.item.item_signal_count = signals;
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "第 {int} 件物品版本应为 {int}")]
fn check_item_version(world: &mut LedgerWorld, n: usize, version: i64) {
    assert_eq!(world.item.nth(n).item.version, version);
}

#[then(expr = "第 {int} 件物品备注应为 {string}")]
fn check_item_note(world: &mut LedgerWorld, n: usize, note: String) {
    assert_eq!(world.item.nth(n).item.note.as_deref(), Some(note.as_str()));
}

#[then(expr = "第 {int} 件物品备注应为空")]
fn check_item_note_empty(world: &mut LedgerWorld, n: usize) {
    assert_eq!(world.item.nth(n).item.note, None);
}

#[when(expr = "记住第 {int} 件物品的创建时间")]
fn remember_item_created_at(world: &mut LedgerWorld, n: usize) {
    world.item.items_list = domain::list_items(&world_conn!(world)).expect("列出物品失败");
    world.item.remembered_item_created_at = Some(world.item.nth(n).item.created_at.clone());
}

#[then(expr = "第 {int} 件物品创建时间应与记住的一致")]
fn check_item_created_at_preserved(world: &mut LedgerWorld, n: usize) {
    let remembered = world
        .item
        .remembered_item_created_at
        .as_deref()
        .unwrap_or_else(|| panic!("没有记住的创建时间（先调「记住…创建时间」步骤）"));
    assert_eq!(
        world.item.nth(n).item.created_at,
        remembered,
        "修改不应改动 created_at"
    );
}

#[then(expr = "第 {int} 件物品成本分解分子应为 {int} 分 ÷ {int} 天 = 每天成本 {float}")]
fn check_item_cost_breakdown(
    world: &mut LedgerWorld,
    n: usize,
    numerator: i64,
    days: i64,
    per_day: f64,
) {
    let entry = world.item.nth(n);
    assert_eq!(entry.numerator_cents, numerator, "成本分解分子不匹配");
    assert_eq!(entry.used_days, days, "成本分解天数不匹配");
    assert!(
        (entry.per_day_cents - per_day).abs() < 1e-6,
        "每天成本不匹配: 期望 {per_day}, 实际 {}",
        entry.per_day_cents
    );
}

#[then(expr = "物品修改应返回错误 {string}")]
fn check_item_update_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}
