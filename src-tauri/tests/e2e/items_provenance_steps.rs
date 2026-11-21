use cucumber::{then, when};

use ledger_infra::error::AppError;
use ledger_item::ItemInput;
use ledger_item::domain::{create_item, update_item};
use ledger_transaction::TransactionInput;
use ledger_transaction::create_transaction_internal;

use crate::items_common::build_input;
use crate::step_inputs::expense_input;
use crate::world::LedgerWorld;

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

#[when(expr = "尝试创建物品 {string} 不关联购买交易")]
fn try_create_item_unlinked(world: &mut LedgerWorld, name: String) {
    let mut signals = 0;
    let result = create_item(
        &world_conn!(world),
        build_input(&name, "2026-03-01".into(), 20_000, "CNY"),
        &mut || signals += 1,
    );
    world.item.item_signal_count = signals;
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "第 {int} 件物品购买日期应为 {string}")]
fn check_item_purchase_date(world: &mut LedgerWorld, n: usize, date: String) {
    assert_eq!(world.item.nth(n).item.purchase_date, date);
}

#[when(expr = "创建支出交易 金额 {int} 币种 {string} 到账户 {string} 日期 {string}")]
fn create_expense_txn_with_currency(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account_name: String,
    date: String,
) {
    let input = TransactionInput {
        currency_code: currency,
        ..expense_input(amount, &world.account_id(&account_name), &date)
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    let write = result.unwrap_or_else(|e| panic!("创建支出交易应成功但失败: {e}"));
    world.txn.last_transaction_id = Some(write.id);
}

#[when(expr = "记住该交易为关联购买交易")]
fn remember_purchase_transaction(world: &mut LedgerWorld) {
    world.item.remembered_purchase_transaction_id = world.txn.last_transaction_id.clone();
}

#[when(expr = "创建物品 {string} 关联该购买交易")]
fn create_item_linked(world: &mut LedgerWorld, name: String) {
    let tx_id = world
        .item
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易（先调「记住该交易为关联购买交易」）"));
    let mut signals = 0;
    let result = create_item(
        &world_conn!(world),
        build_linked_input(&name, &tx_id),
        &mut || signals += 1,
    );
    match result {
        Ok(id) => {
            world.item.last_item_id = Some(id);
            world.item.item_signal_count = signals;
        }
        Err(e) => panic!("创建物品应成功但失败: {e}"),
    }
}

#[when(expr = "尝试创建物品 {string} 关联该购买交易")]
fn try_create_item_linked(world: &mut LedgerWorld, name: String) {
    let tx_id = world
        .item
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易（先调「记住该交易为关联购买交易」）"));
    let mut signals = 0;
    let result = create_item(
        &world_conn!(world),
        build_linked_input(&name, &tx_id),
        &mut || signals += 1,
    );
    world.item.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

#[when(expr = "尝试创建物品 {string} 关联不存在的购买交易")]
fn try_create_item_linked_missing(world: &mut LedgerWorld, name: String) {
    let mut signals = 0;
    let result = create_item(
        &world_conn!(world),
        build_linked_input(&name, "no-such-transaction"),
        &mut || signals += 1,
    );
    world.item.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

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
        .item
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易（先调「记住该交易为关联购买交易」）"));
    let mut signals = 0;
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可修改"));
    let mut input = build_linked_input(&name, &tx_id);
    input.purchase_date = date;
    input.total_cost_cents = cost_cents;
    input.currency_code = currency;
    input.note = if note.is_empty() { None } else { Some(note) };
    let result = update_item(&world_conn!(world), &id, input, &mut || signals += 1);
    match result {
        Ok(()) => world.item.item_signal_count = signals,
        Err(e) => panic!("修改物品应成功但失败: {e}"),
    }
}

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
        .item
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易"));
    let mut signals = 0;
    let id = world
        .item
        .last_item_id
        .clone()
        .unwrap_or_else(|| panic!("没有已创建的物品可修改"));
    let mut input = build_linked_input(&name, &tx_id);
    input.purchase_date = date;
    input.total_cost_cents = cost_cents;
    input.currency_code = currency;
    input.note = if note.is_empty() { None } else { Some(note) };
    let result = update_item(&world_conn!(world), &id, input, &mut || signals += 1);
    world.item.item_signal_count = signals;
    world.last_error = match result {
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "第 {int} 件物品关联购买交易应为记住的交易")]
fn check_item_linked_transaction(world: &mut LedgerWorld, n: usize) {
    let expected = world
        .item
        .remembered_purchase_transaction_id
        .clone()
        .unwrap_or_else(|| panic!("没有记住的关联购买交易"));
    assert_eq!(
        world.item.nth(n).item.purchase_transaction_id.as_deref(),
        Some(expected.as_str()),
        "物品溯源应指向记住的关联购买交易"
    );
}
