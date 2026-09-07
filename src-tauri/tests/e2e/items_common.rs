//! 物品（Item）e2e 步骤的共享辅助（build_input）：
//! 供创建/更新/溯源关联模块复用；函数体与原 `items_steps.rs` 一致（纯搬迁，一字不改）。
//! 列表快照读取已收编为物品组分组方法（`world.item.nth`，见 world.rs）。

use tauri_app_lib::item::ItemInput;

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
