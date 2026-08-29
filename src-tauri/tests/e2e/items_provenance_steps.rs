//! 物品（Item）BDD 步骤 · 溯源关联主题（issue #207 / ADR-0025、issue #119）：
//! 创建唯一入口守卫与 source_transaction_id 关联语义。
//!
//! 创建必关联购买交易（不关联被拒、不落库）；关联创建/编辑时后端自动带出
//! 日期与基础成本；校验交易存在且为 expense；同一交易溯源唯一（创建与更新
//! 守卫共用，只看未删除物品）。

use cucumber::{then, when};

use tauri_app_lib::commands::item::{create_item_internal, update_item_internal};
use tauri_app_lib::commands::transactions::create_transaction_internal;
use tauri_app_lib::error::AppError;
use tauri_app_lib::models::{ItemInput, TransactionInput};
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::items_common::{build_input, nth_item};
use crate::world::LedgerWorld;

/// 关联购买交易的入参：日期/成本/币种填入**故意错误的占位值**，
/// 断言「自动带出」这一外部行为（后端必须用交易值覆盖占位值）。
fn build_linked_input(name: &str, tx_id: &str) -> ItemInput {
    ItemInput {
        name: name.into(),
        purchase_date: "1970-01-01".into(),
        total_cost_cents: 1,
        currency_code: "CNY".into(),
        note: None,
        purchase_transaction_id: Some(tx_id.into()),
    }
}

/// 尝试不关联购买交易创建物品并捕获错误（issue #207 溯源守卫拒绝路径：
/// 创建请求缺溯源直接拒绝，不发失效信号、不落库）。
#[when(expr = "尝试创建物品 {string} 不关联购买交易")]
fn try_create_item_unlinked(world: &mut LedgerWorld, name: String) {
    let mut signals = 0;
    let result = create_item_internal(
        &world_conn!(world),
        build_input(&name, "2026-03-01".into(), 20_000, "CNY"),
        &mut || signals += 1,
    );
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "第 {int} 件物品购买日期应为 {string}")]
fn check_item_purchase_date(world: &mut LedgerWorld, n: usize, date: String) {
    assert_eq!(nth_item(world, n).item.purchase_date, date);
}

// ---------------------------------------------------------------------------
// 关联购买交易（issue #119）：自动带出日期/成本，存溯源，校验存在且为 expense
// ---------------------------------------------------------------------------

/// 创建一笔外币支出交易（通用「创建交易」步骤固定 CNY，此处补币种参数）。
#[when(expr = "创建支出交易 金额 {int} 币种 {string} 到账户 {string} 日期 {string}")]
fn create_expense_txn_with_currency(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account_name: String,
    date: String,
) {
    let input = TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Expense,
        amount_cents: amount,
        currency_code: currency,
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    let id = result.unwrap_or_else(|e| panic!("创建支出交易应成功但失败: {e}"));
    world.last_transaction_id = Some(id);
}

/// 记住最近创建的交易为关联购买交易（后续「关联该购买交易」步骤引用它）。
#[when(expr = "记住该交易为关联购买交易")]
fn remember_purchase_transaction(world: &mut LedgerWorld) {
    world.remembered_purchase_transaction_id = world.last_transaction_id.clone();
}

/// 创建物品并关联记住的购买交易：入参日期/成本为占位值，
/// 后端必须用交易值覆盖（自动带出）。
#[when(expr = "创建物品 {string} 关联该购买交易")]
fn create_item_linked(world: &mut LedgerWorld, name: String) {
    let tx_id = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易（先调「记住该交易为关联购买交易」）"));
    let mut signals = 0;
    let result = create_item_internal(
        &world_conn!(world),
        build_linked_input(&name, &tx_id),
        &mut || signals += 1,
    );
    match result {
        Ok(id) => {
            world.last_item_id = Some(id);
            world.item_signal_count = signals;
        }
        Err(e) => panic!("创建物品应成功但失败: {e}"),
    }
}

/// 尝试创建关联记住交易的物品并捕获错误（非 expense 报错路径）。
#[when(expr = "尝试创建物品 {string} 关联该购买交易")]
fn try_create_item_linked(world: &mut LedgerWorld, name: String) {
    let tx_id = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易（先调「记住该交易为关联购买交易」）"));
    let mut signals = 0;
    let result = create_item_internal(
        &world_conn!(world),
        build_linked_input(&name, &tx_id),
        &mut || signals += 1,
    );
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 尝试创建关联不存在交易的物品并捕获错误（固定假 id 走不存在报错路径）。
#[when(expr = "尝试创建物品 {string} 关联不存在的购买交易")]
fn try_create_item_linked_missing(world: &mut LedgerWorld, name: String) {
    let mut signals = 0;
    let result = create_item_internal(
        &world_conn!(world),
        build_linked_input(&name, "no-such-transaction"),
        &mut || signals += 1,
    );
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 修改最近创建的物品并关联记住的购买交易。入参日期/成本为占位值：
/// 新关联/换关时后端必须用交易值覆盖（自动带出）；维持既有关联时则原样落库，
/// 两种语义由不同场景分别断言。
#[when(
    expr = "修改物品名称为 {string} 购买日期 {string} 总成本 {int} 币种 {string} 关联该购买交易 备注为 {string}"
)]
fn update_item_linked(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let tx_id = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易（先调「记住该交易为关联购买交易」）"));
    let mut signals = 0;
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可修改"));
    let mut input = build_linked_input(&name, &tx_id);
    input.purchase_date = date;
    input.total_cost_cents = cost_cents;
    input.currency_code = currency;
    input.note = if note.is_empty() { None } else { Some(note) };
    let result = update_item_internal(&world_conn!(world), &id, input, &mut || signals += 1);
    match result {
        Ok(()) => world.item_signal_count = signals,
        Err(e) => panic!("修改物品应成功但失败: {e}"),
    }
}

/// 尝试修改最近创建的物品并关联记住的购买交易（捕获错误，溯源唯一拒绝路径）。
#[when(
    expr = "尝试修改物品名称为 {string} 购买日期 {string} 总成本 {int} 币种 {string} 关联该购买交易 备注为 {string}"
)]
fn try_update_item_linked(
    world: &mut LedgerWorld,
    name: String,
    date: String,
    cost_cents: i64,
    currency: String,
    note: String,
) {
    let tx_id = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易"));
    let mut signals = 0;
    let id = world
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可修改"));
    let mut input = build_linked_input(&name, &tx_id);
    input.purchase_date = date;
    input.total_cost_cents = cost_cents;
    input.currency_code = currency;
    input.note = if note.is_empty() { None } else { Some(note) };
    let result = update_item_internal(&world_conn!(world), &id, input, &mut || signals += 1);
    world.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

/// 断言第 n 件物品的溯源指向记住的关联购买交易。
#[then(expr = "第 {int} 件物品关联购买交易应为记住的交易")]
fn check_item_linked_transaction(world: &mut LedgerWorld, n: usize) {
    let expected = world
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易"));
    assert_eq!(
        nth_item(world, n).item.purchase_transaction_id.as_deref(),
        Some(expected.as_str()),
        "物品溯源应指向记住的关联购买交易"
    );
}
