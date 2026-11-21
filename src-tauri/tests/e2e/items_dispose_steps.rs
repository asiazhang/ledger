use cucumber::{then, when};

use ledger_item::ItemDisposeInput;
use ledger_item::domain::{delete_item, dispose_item};

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

fn find_item_id_by_name(conn: &rusqlite::Connection, name: &str) -> String {
    conn.query_row(
        "SELECT id FROM items WHERE name=?1 AND is_deleted=0",
        rusqlite::params![name],
        |r| r.get(0),
    )
    .unwrap_or_else(|e| panic!("按名称 {name} 查找未删除物品失败: {e}"))
}

#[when(expr = "软删除物品 {string}")]
fn soft_delete_item(world: &mut LedgerWorld, name: String) {
    let id = find_item_id_by_name(&world_conn!(world), &name);
    let mut signals = 0;
    let result = delete_item(&world_conn!(world), &id, &mut || signals += 1);
    world.item.item_signal_count = signals;
    if let Err(e) = result {
        panic!("软删除物品 {name} 应成功但失败: {e}");
    }
}

#[then(expr = "删除后应发出 {int} 次失效信号")]
fn check_item_delete_signals(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.item.item_signal_count, expected,
        "删除失效信号次数不匹配（生产路径对应 ledger:changed）"
    );
}

#[then(expr = "物品 {string} 行仍存在且 is_deleted=1")]
fn check_item_row_soft_deleted(world: &mut LedgerWorld, name: String) {
    let (count, is_deleted): (i64, i64) = world_conn!(world)
        .query_row(
            "SELECT COUNT(*), MAX(is_deleted) FROM items WHERE name=?1",
            rusqlite::params![name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("查询物品行失败");
    assert_eq!(count, 1, "软删除后行应保留在库（不物理移除）");
    assert_eq!(is_deleted, 1, "软删除应打 is_deleted=1 标记");
}

#[when(expr = "尝试软删除不存在的物品")]
fn try_delete_missing_item(world: &mut LedgerWorld) {
    let mut signals = 0;
    let result = delete_item(&world_conn!(world), "no-such-item-id", &mut || signals += 1);
    world.item.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "物品删除应返回错误 {string}")]
fn check_item_delete_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

fn dispose_by_id(
    world: &mut LedgerWorld,
    id: &str,
    input: ItemDisposeInput,
) -> Result<(), ledger_infra::error::AppError> {
    let mut signals = 0;
    let result = dispose_item(&world_conn!(world), id, input, &mut || signals += 1);
    world.item.item_signal_count = signals;
    result
}

#[when(expr = "处置物品 处置日期 {string} 残值 {int}")]
fn dispose_item_with_residual(world: &mut LedgerWorld, date: String, residual: i64) {
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可处置"));
    assert_dispose_ok(world, &id, date, Some(residual));
}

#[when(expr = "处置物品 处置日期 {string} 不填残值")]
fn dispose_item_without_residual(world: &mut LedgerWorld, date: String) {
    let id = world
        .item
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

#[when(expr = "尝试处置物品 处置日期 {string} 残值 {int}")]
fn try_dispose_item(world: &mut LedgerWorld, date: String, residual: i64) {
    let id = world
        .item
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

#[then(expr = "第 {int} 件物品处置日期应为 {string} 残值应为 {int}")]
fn check_item_disposal(world: &mut LedgerWorld, n: usize, date: String, residual: i64) {
    let item = &world.item.nth(n).item;
    assert_eq!(item.disposal_date.as_deref(), Some(date.as_str()));
    assert_eq!(item.residual_value_cents, Some(residual));
}

#[then(expr = "第 {int} 件物品处置日期应为 {string} 残值应为空")]
fn check_item_disposal_no_residual(world: &mut LedgerWorld, n: usize, date: String) {
    let item = &world.item.nth(n).item;
    assert_eq!(item.disposal_date.as_deref(), Some(date.as_str()));
    assert_eq!(item.residual_value_cents, None);
}

#[then(expr = "物品处置应返回错误 {string}")]
fn check_item_dispose_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}
