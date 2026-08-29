//! 物品（Item）BDD 步骤 · 处置主题（issue #118 软删除、issue #120 生命周期处置）：
//! 软删除只打标记不物理移除；处置带出处置日期/残值并重算摊薄（分子扣残值），
//! 处置校验（日期晚于今天/早于购买/格式/负残值）与不存在路径。

use cucumber::{then, when};

use tauri_app_lib::commands::item::{delete_item_internal, dispose_item_internal};
use tauri_app_lib::models::ItemDisposeInput;

use crate::common::assert_last_error_contains;
use crate::items_common::nth_item;
use crate::world::LedgerWorld;

/// 按名称查未删除物品 id 的辅助（失败即 panic，场景数据自洽由写步骤保证）。
fn find_item_id_by_name(conn: &rusqlite::Connection, name: &str) -> String {
    conn.query_row(
        "SELECT id FROM items WHERE name=?1 AND is_deleted=0",
        rusqlite::params![name],
        |r| r.get(0),
    )
    .unwrap_or_else(|e| panic!("按名称 {name} 查找未删除物品失败: {e}"))
}

/// 软删除指定名称的物品（要求成功；记录失效信号次数）。
#[when(expr = "软删除物品 {string}")]
fn soft_delete_item(world: &mut LedgerWorld, name: String) {
    let id = find_item_id_by_name(&world.conn, &name);
    let mut signals = 0;
    let result = delete_item_internal(&world.conn, &id, &mut || signals += 1);
    world.item_signal_count = signals;
    if let Err(e) = result {
        panic!("软删除物品 {name} 应成功但失败: {e}");
    }
}

#[then(expr = "删除后应发出 {int} 次失效信号")]
fn check_item_delete_signals(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.item_signal_count, expected,
        "删除失效信号次数不匹配（生产路径对应 ledger:changed）"
    );
}

/// 直接查库断言软删除语义：行未被物理移除，仅打 `is_deleted=1` 标记。
#[then(expr = "物品 {string} 行仍存在且 is_deleted=1")]
fn check_item_row_soft_deleted(world: &mut LedgerWorld, name: String) {
    let (count, is_deleted): (i64, i64) = world
        .conn
        .query_row(
            "SELECT COUNT(*), MAX(is_deleted) FROM items WHERE name=?1",
            rusqlite::params![name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("查询物品行失败");
    assert_eq!(count, 1, "软删除后行应保留在库（不物理移除）");
    assert_eq!(is_deleted, 1, "软删除应打 is_deleted=1 标记");
}

/// 尝试删除不存在的物品 id（捕获错误供「应返回错误」断言）。
#[when(expr = "尝试软删除不存在的物品")]
fn try_delete_missing_item(world: &mut LedgerWorld) {
    let mut signals = 0;
    let result = delete_item_internal(&world.conn, "no-such-item-id", &mut || signals += 1);
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "物品删除应返回错误 {string}")]
fn check_item_delete_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

/// 处置物品的共用入口：记录失效信号次数并返回结果（成功/错误均不 panic）。
fn dispose_by_id(
    world: &mut LedgerWorld,
    id: &str,
    input: ItemDisposeInput,
) -> Result<(), tauri_app_lib::error::AppError> {
    let mut signals = 0;
    let result = dispose_item_internal(&world.conn, id, input, &mut || signals += 1);
    world.item_signal_count = signals;
    result
}

/// 处置最近创建的物品（`world.last_item_id`），要求成功。
#[when(expr = "处置物品 处置日期 {string} 残值 {int}")]
fn dispose_item_with_residual(world: &mut LedgerWorld, date: String, residual: i64) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可处置"));
    assert_dispose_ok(world, &id, date, Some(residual));
}

/// 处置最近创建的物品，不填残值（残值可选语义）。
#[when(expr = "处置物品 处置日期 {string} 不填残值")]
fn dispose_item_without_residual(world: &mut LedgerWorld, date: String) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可处置"));
    assert_dispose_ok(world, &id, date, None);
}

fn assert_dispose_ok(world: &mut LedgerWorld, id: &str, date: String, residual: Option<i64>) {
    if let Err(e) = dispose_by_id(
        world,
        id,
        ItemDisposeInput {
            disposal_date: date,
            residual_value_cents: residual,
        },
    ) {
        panic!("处置物品应成功但失败: {e}");
    }
}

/// 尝试处置最近创建的物品并捕获错误（供「应返回错误」断言）。
#[when(expr = "尝试处置物品 处置日期 {string} 残值 {int}")]
fn try_dispose_item(world: &mut LedgerWorld, date: String, residual: i64) {
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| "no-such-item".into());
    world.last_error = match dispose_by_id(
        world,
        &id,
        ItemDisposeInput {
            disposal_date: date,
            residual_value_cents: Some(residual),
        },
    ) {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

/// 尝试处置不存在的物品 id（固定假 id 走 NotFound 报错路径）。
#[when(expr = "尝试处置不存在的物品")]
fn try_dispose_missing_item(world: &mut LedgerWorld) {
    world.last_error = match dispose_by_id(
        world,
        "no-such-item-id",
        ItemDisposeInput {
            disposal_date: "2026-01-01".into(),
            residual_value_cents: None,
        },
    ) {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

/// 断言第 n 件物品的处置日期与残值读回。
#[then(expr = "第 {int} 件物品处置日期应为 {string} 残值应为 {int}")]
fn check_item_disposal(world: &mut LedgerWorld, n: usize, date: String, residual: i64) {
    let item = &nth_item(world, n).item;
    assert_eq!(item.disposal_date.as_deref(), Some(date.as_str()));
    assert_eq!(item.residual_value_cents, Some(residual));
}

/// 断言第 n 件物品处置日期读回且残值为空（可选残值语义）。
#[then(expr = "第 {int} 件物品处置日期应为 {string} 残值应为空")]
fn check_item_disposal_no_residual(world: &mut LedgerWorld, n: usize, date: String) {
    let item = &nth_item(world, n).item;
    assert_eq!(item.disposal_date.as_deref(), Some(date.as_str()));
    assert_eq!(item.residual_value_cents, None);
}

/// 复用「应返回错误」断言（同一 seam：world.last_error 包含片段）。
#[then(expr = "物品处置应返回错误 {string}")]
fn check_item_dispose_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}
