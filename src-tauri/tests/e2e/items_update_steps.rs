//! 物品（Item）BDD 步骤 · 更新主题（issue #117）：修改物品与修改路径校验。
//!
//! 断言修改读回、版本递增、审计字段（created_at）保留、备注清除语义、
//! 修改后每天成本按新口径重算（成本分解三元组）、修改失败不落库不发信号。

use cucumber::{then, when};

use tauri_app_lib::error::AppError;
use tauri_app_lib::item::cost;
use tauri_app_lib::item::domain;
use tauri_app_lib::models::ItemInput;

use crate::common::assert_last_error_contains;
use crate::items_common::{build_input, nth_item};
use crate::world::LedgerWorld;

/// 填备注：空字符串规为清除（None），其余原样。
fn with_note(mut input: ItemInput, note: &str) -> ItemInput {
    input.note = if note.is_empty() {
        None
    } else {
        Some(note.to_string())
    };
    input
}

/// 修改最近创建的物品（`world.last_item_id`）并要求成功；备注空字符串规为清除（None）。
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
        Ok(()) => world.item_signal_count = signals,
        Err(e) => panic!("修改物品应成功但失败: {e}"),
    }
}

/// 修改物品（购买日期 = 今天前 N 天；日期口径同创建步骤，保证天数可静态断言）。
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

/// 尝试修改物品并捕获错误（供「应返回错误」断言，与创建场景同一 seam）。
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
    // 不存在场景传固定假 id，真实走到 query_one 落空的 NotFound 路径
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| "no-such-item".into());
    let result = domain::update_item(
        &world_conn!(world),
        &id,
        with_note(build_input(&name, date, cost_cents, &currency), &note),
        &mut || signals += 1,
    );
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "第 {int} 件物品版本应为 {int}")]
fn check_item_version(world: &mut LedgerWorld, n: usize, version: i64) {
    assert_eq!(nth_item(world, n).item.version, version);
}

#[then(expr = "第 {int} 件物品备注应为 {string}")]
fn check_item_note(world: &mut LedgerWorld, n: usize, note: String) {
    assert_eq!(nth_item(world, n).item.note.as_deref(), Some(note.as_str()));
}

#[then(expr = "第 {int} 件物品备注应为空")]
fn check_item_note_empty(world: &mut LedgerWorld, n: usize) {
    assert_eq!(nth_item(world, n).item.note, None);
}

#[when(expr = "记住第 {int} 件物品的创建时间")]
fn remember_item_created_at(world: &mut LedgerWorld, n: usize) {
    world.items_list = domain::list_items(&world_conn!(world)).expect("列出物品失败");
    world.remembered_item_created_at = Some(nth_item(world, n).item.created_at.clone());
}

#[then(expr = "第 {int} 件物品创建时间应与记住的一致")]
fn check_item_created_at_preserved(world: &mut LedgerWorld, n: usize) {
    let remembered = world
        .remembered_item_created_at
        .as_deref()
        .unwrap_or_else(|| panic!("没有记住的创建时间（先调「记住…创建时间」步骤）"));
    assert_eq!(
        nth_item(world, n).item.created_at,
        remembered,
        "修改不应改动 created_at"
    );
}

/// 成本分解断言：分子 ÷ 天数 = 每天成本（详情视图展示的口径三元组）。
#[then(expr = "第 {int} 件物品成本分解分子应为 {int} 分 ÷ {int} 天 = 每天成本 {float}")]
fn check_item_cost_breakdown(
    world: &mut LedgerWorld,
    n: usize,
    numerator: i64,
    days: i64,
    per_day: f64,
) {
    let entry = nth_item(world, n);
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
