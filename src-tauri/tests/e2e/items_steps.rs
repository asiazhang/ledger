//! 物品（Item）BDD 步骤（issue #115 / #118 / spec #113）：创建、列出与软删除物品。
//!
//! 经 `commands::item` 的 `*_internal` seam 断言外部可观察行为：
//! 创建读回、每天成本（`item::cost` 口径）、金额折算、写后发失效信号
//! （notify 注入，生产路径发 `ledger:changed`）、软删除后标准列表过滤。

use cucumber::{then, when};
use tauri_app_lib::commands::item::{
    create_item_internal, delete_item_internal, list_items_internal,
};
use tauri_app_lib::error::AppError;
use tauri_app_lib::item::cost;
use tauri_app_lib::models::{ItemInput, ItemStatus};

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

fn build_input(name: &str, date: String, cost_cents: i64, currency: &str) -> ItemInput {
    ItemInput {
        name: name.into(),
        purchase_date: date,
        total_cost_cents: cost_cents,
        currency_code: currency.into(),
        note: None,
    }
}

/// 创建物品并要求成功；记录失效信号次数（写后发 `ledger:changed` 的 seam 断言）。
#[when(expr = "创建物品 {string} 购买日期 {string} 总成本 {int} 币种 {string}")]
fn create_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
) {
    let mut signals = 0;
    let result = create_item_internal(
        &world.conn,
        build_input(&name, date, cost_cents, &currency),
        &mut || signals += 1,
    );
    match result {
        Ok(id) => {
            world.last_item_id = Some(id);
            world.item_signal_count = signals;
        }
        Err(AppError::Invalid(msg)) => panic!("创建物品应成功但失败: {msg}"),
        Err(e) => panic!("创建物品应成功但失败: {e}"),
    }
}

/// 创建物品（购买日期 = 今天，本地时区日历日，同 `item::cost::today` 口径）。
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

/// 创建物品（购买日期 = 今天前 N 天；N=9 → 含起止两端共 10 天）。
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

/// 尝试创建物品并捕获错误（供「应返回错误」断言，与交易场景同一 seam）。
#[when(expr = "尝试创建物品 {string} 购买日期 {string} 总成本 {int} 币种 {string}")]
fn try_create_item(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
) {
    let mut signals = 0;
    let result = create_item_internal(
        &world.conn,
        build_input(&name, date, cost_cents, &currency),
        &mut || signals += 1,
    );
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 刷新物品列表快照并断言件数。
#[then(expr = "物品列表应包含 {int} 件物品")]
fn refresh_and_check_item_count(world: &mut LedgerWorld, expected: usize) {
    world.items_list = list_items_internal(&world.conn).expect("列出物品失败");
    assert_eq!(
        world.items_list.len(),
        expected,
        "物品件数不匹配: {:?}",
        world
            .items_list
            .iter()
            .map(|i| &i.item.name)
            .collect::<Vec<_>>()
    );
}

/// 取第 n 件（1 起）物品快照的辅助。
fn nth_item(world: &LedgerWorld, n: usize) -> &tauri_app_lib::models::ItemWithDailyCost {
    world
        .items_list
        .get(n - 1)
        .unwrap_or_else(|| panic!("物品列表第 {n} 件不存在"))
}

#[then(expr = "第 {int} 件物品名称应为 {string}")]
fn check_item_name(world: &mut LedgerWorld, n: usize, name: String) {
    assert_eq!(nth_item(world, n).item.name, name);
}

#[then(expr = "第 {int} 件物品总成本应为 {int} 币种应为 {string} 本位币成本应为 {int}")]
fn check_item_amounts(
    world: &mut LedgerWorld,
    n: usize,
    cost_cents: i64,
    currency: String,
    native_cents: i64,
) {
    let item = &nth_item(world, n).item;
    assert_eq!(item.total_cost_cents, cost_cents);
    assert_eq!(item.currency_code, currency);
    assert_eq!(item.cost_native_cents, native_cents);
}

#[then(expr = "第 {int} 件物品状态应为 {string}")]
fn check_item_status(world: &mut LedgerWorld, n: usize, status: String) {
    let parsed = ItemStatus::parse(&status).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(nth_item(world, n).item.status, parsed);
}

#[then(expr = "第 {int} 件物品已用天数应为 {int} 每天成本应为 {float}")]
fn check_item_daily_cost(world: &mut LedgerWorld, n: usize, days: i64, per_day: f64) {
    let entry = nth_item(world, n);
    assert_eq!(entry.used_days, days);
    assert!(
        (entry.per_day_cents - per_day).abs() < 1e-6,
        "每天成本不匹配: 期望 {per_day}, 实际 {}",
        entry.per_day_cents
    );
}

#[then(expr = "第 {int} 件物品应有唯一 ID 与审计字段")]
fn check_item_audit_fields(world: &mut LedgerWorld, n: usize) {
    let item = &nth_item(world, n).item;
    assert!(!item.id.is_empty(), "物品 id 不应为空");
    assert_eq!(item.version, 1, "新物品 version 应为 1");
    assert!(!item.device_id.is_empty(), "device_id 不应为空");
    assert!(!item.created_at.is_empty(), "created_at 不应为空");
    assert!(!item.updated_at.is_empty(), "updated_at 不应为空");
    assert!(!item.is_deleted);
}

#[then(expr = "创建后应发出 {int} 次失效信号")]
fn check_item_signals(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.item_signal_count, expected,
        "失效信号次数不匹配（生产路径对应 ledger:changed）"
    );
}

#[then(expr = "未发出失效信号")]
fn check_no_item_signals(world: &mut LedgerWorld) {
    assert_eq!(world.item_signal_count, 0, "不应发出失效信号");
}

/// 复用交易的「应返回错误」断言（同一 seam：world.last_error 包含片段）。
#[then(expr = "物品创建应返回错误 {string}")]
fn check_item_error(world: &mut LedgerWorld, expected: String) {
    assert_last_error_contains(world, &expected);
}

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
