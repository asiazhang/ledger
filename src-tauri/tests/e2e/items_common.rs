//! 物品（Item）e2e 步骤的共享辅助（build_input / nth_item）：
//! build_input 供创建/更新/溯源关联模块复用，nth_item 供创建/更新/溯源关联/处置
//! 模块的列表断言复用；函数体与原 `items_steps.rs` 一致（纯搬迁，一字不改）。

use tauri_app_lib::item::ItemInput;

use crate::world::LedgerWorld;

pub fn build_input(name: &str, date: String, cost_cents: i64, currency: &str) -> ItemInput {
    ItemInput {
        name: name.into(),
        purchase_date: date,
        total_cost_cents: cost_cents,
        currency_code: currency.into(),
        note: None,
        purchase_transaction_id: None,
    }
}

/// 取第 n 件（1 起）物品快照的辅助。
pub fn nth_item(world: &LedgerWorld, n: usize) -> &tauri_app_lib::item::ItemWithDailyCost {
    world
        .items_list
        .get(n - 1)
        .unwrap_or_else(|| panic!("物品列表第 {n} 件不存在"))
}
