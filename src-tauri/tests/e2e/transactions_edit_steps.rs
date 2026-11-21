use cucumber::{then, when};
use rusqlite::params;

use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;

use crate::common::{capture_expected_error, instrument_id_by_symbol, query_all_transactions};
use crate::step_inputs::{existing_input, expense_input, parse_kind, trade_input};
use crate::step_verbs::{
    create_transaction_verb, delete_transaction_verb, try_update_transaction_verb,
    update_transaction_verb,
};
use crate::world::LedgerWorld;

#[when(expr = "修改最近交易 类型 {string} 金额 {int} 日期 {string} 备注 {string}")]
fn update_last_txn(world: &mut LedgerWorld, kind: String, amount: i64, date: String, note: String) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可修改的交易");
    let existing = world
        .txn
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在");
    let input = TransactionInput {
        kind: parse_kind(&kind),
        amount_cents: amount,
        note: Some(note),
        date,
        ..existing_input(existing)
    };
    update_transaction_verb(world, &id, input);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "尝试修改最近交易为转账 金额 {int} 日期 {string}")]
fn try_update_last_to_transfer(world: &mut LedgerWorld, amount: i64, date: String) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可修改的交易");
    let existing = world
        .txn
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在");
    let input = TransactionInput {
        kind: TransactionKind::Transfer,
        amount_cents: amount,
        to_account_id: None,
        note: None,
        date,
        ..existing_input(existing)
    };
    let result = try_update_transaction_verb(world, &id, input);
    capture_expected_error(world, result);
}

#[when(expr = "删除最近交易")]
fn delete_last_txn(world: &mut LedgerWorld) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可删除的交易");
    delete_transaction_verb(world, &id);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "删除买入交易 {string}")]
fn delete_buy_by_symbol(world: &mut LedgerWorld, symbol: String) {
    let id = trade_txn_id(world, &symbol, "buy");
    delete_transaction_verb(world, &id);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

pub(crate) fn trade_txn_id(world: &LedgerWorld, symbol: &str, action: &str) -> String {
    world_conn!(world)
        .query_row(
            "SELECT st.transaction_id FROM security_transactions st \
             JOIN instruments i ON i.id = st.instrument_id \
             WHERE i.symbol=?1 AND st.action=?2",
            params![symbol, action],
            |r| r.get(0),
        )
        .expect("未找到对应买卖交易")
}

fn insert_trade_for_edit(
    world: &mut LedgerWorld,
    kind: TransactionKind,
    symbol: &str,
    quantity: i64,
    price_cents: i64,
    account_name: &str,
    date: &str,
) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), symbol);
    let account_id = world.account_id(account_name);
    create_transaction_verb(
        world,
        trade_input(
            kind,
            &instrument_id,
            quantity as f64,
            price_cents,
            &account_id,
            date,
        ),
    );
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "买入标的 {string} 数量 {int} 单价 {int} 到投资账户 {string}")]
fn buy_for_edit(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    account_name: String,
) {
    insert_trade_for_edit(
        world,
        TransactionKind::Buy,
        &symbol,
        quantity,
        price_cents,
        &account_name,
        "2026-01-10",
    );
}

#[when(expr = "卖出标的 {string} 数量 {int} 单价 {int} 从投资账户 {string}")]
fn sell_for_edit(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    account_name: String,
) {
    insert_trade_for_edit(
        world,
        TransactionKind::Sell,
        &symbol,
        quantity,
        price_cents,
        &account_name,
        "2026-01-20",
    );
}

fn trade_edit_input(
    world: &LedgerWorld,
    kind: TransactionKind,
    id: &str,
    quantity: i64,
    price_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    let instrument_id: String = world_conn!(world)
        .query_row(
            "SELECT instrument_id FROM security_transactions WHERE transaction_id=?1",
            params![id],
            |r| r.get(0),
        )
        .expect("该交易无买卖明细");
    let existing = world
        .txn
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在");
    TransactionInput {
        kind,
        amount_cents: 0,
        instrument_id: Some(instrument_id),
        quantity: Some(quantity as f64),
        price_cents: Some(price_cents),
        fee_cents: Some(fee_cents),
        ..existing_input(existing)
    }
}

#[when(expr = "修改买入交易 {string} 数量 {int} 单价 {int} 手续费 {int}")]
fn update_buy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    fee_cents: i64,
) {
    let id = trade_txn_id(world, &symbol, "buy");
    let input = trade_edit_input(
        world,
        TransactionKind::Buy,
        &id,
        quantity,
        price_cents,
        fee_cents,
    );
    update_transaction_verb(world, &id, input);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "尝试修改买入交易 {string} 数量 {int} 单价 {int}")]
fn try_update_partially_sold_buy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
) {
    let id = trade_txn_id(world, &symbol, "buy");
    let input = trade_edit_input(world, TransactionKind::Buy, &id, quantity, price_cents, 0);
    let result = try_update_transaction_verb(world, &id, input);
    capture_expected_error(world, result);
}

#[when(expr = "修改卖出交易 {string} 数量 {int} 单价 {int} 手续费 {int}")]
fn update_sell(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    fee_cents: i64,
) {
    let id = trade_txn_id(world, &symbol, "sell");
    let input = trade_edit_input(
        world,
        TransactionKind::Sell,
        &id,
        quantity,
        price_cents,
        fee_cents,
    );
    update_transaction_verb(world, &id, input);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "尝试修改已删除的交易 金额 {int} 日期 {string}")]
fn try_update_deleted_txn(world: &mut LedgerWorld, amount: i64, date: String) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可修改的交易");
    let input = expense_input(amount, "acc-x", &date);
    let result = try_update_transaction_verb(world, &id, input);
    capture_expected_error(world, result);
}

#[then(expr = "第 {int} 条交易版本应为 {int}")]
fn check_txn_version(world: &mut LedgerWorld, index: i64, expected_version: i64) {
    let idx = (index - 1) as usize;
    assert!(
        idx < world.txn.transactions_list.len(),
        "交易列表只有 {} 条，无法访问第 {index} 条",
        world.txn.transactions_list.len()
    );
    assert_eq!(
        world.txn.transactions_list[idx].version, expected_version,
        "交易版本号不匹配"
    );
}

#[then(expr = "标的 {string} 持仓数量应为 {int}")]
fn assert_holding_quantity(world: &mut LedgerWorld, symbol: String, expected: i64) {
    let quantity: f64 = world_conn!(world)
        .query_row(
            "SELECT remaining_quantity FROM security_lots \
             WHERE instrument_id = (SELECT id FROM instruments WHERE symbol=?1)",
            params![symbol],
            |r| r.get(0),
        )
        .expect("该标的的持仓批次不存在");
    assert!(
        (quantity - expected as f64).abs() < 1e-9,
        "持仓数量不符: 期望 {expected}，实际 {quantity}"
    );
}

#[then(expr = "标的 {string} 持仓批次应为 {int} 条")]
fn assert_lot_count(world: &mut LedgerWorld, symbol: String, expected: i64) {
    let count: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM security_lots \
             WHERE instrument_id = (SELECT id FROM instruments WHERE symbol=?1)",
            params![symbol],
            |r| r.get(0),
        )
        .expect("查询持仓批次失败");
    assert_eq!(count, expected, "持仓批次数不符");
}

fn assert_trade_detail_of(
    world: &LedgerWorld,
    symbol: &str,
    action: &str,
    quantity: i64,
    price_cents: i64,
    fee_cents: i64,
) {
    let id = trade_txn_id(world, symbol, action);
    let expected_instrument_id = instrument_id_by_symbol(&world_conn!(world), symbol);
    let (instrument_id, trade_quantity, trade_price, trade_fee): (String, f64, i64, i64) =
        world_conn!(world)
            .query_row(
                "SELECT st.instrument_id, st.quantity, st.price_cents, st.fee_cents \
             FROM security_transactions st JOIN instruments i ON i.id = st.instrument_id \
             WHERE st.transaction_id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("读取买卖明细失败");
    assert_eq!(instrument_id, expected_instrument_id, "标的不符");
    assert!(
        (trade_quantity - quantity as f64).abs() < 1e-9,
        "数量不符: 期望 {quantity}，实际 {trade_quantity}"
    );
    assert_eq!(trade_price, price_cents, "单价不符");
    assert_eq!(trade_fee, fee_cents, "手续费不符");
}

#[then(expr = "该买入明细应为 标的 {string} 数量 {int} 单价 {int} 手续费 {int}")]
fn assert_buy_detail(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    fee_cents: i64,
) {
    assert_trade_detail_of(world, &symbol, "buy", quantity, price_cents, fee_cents);
}

#[then(expr = "该卖出明细应为 标的 {string} 数量 {int} 单价 {int} 手续费 {int}")]
fn assert_sell_detail(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    fee_cents: i64,
) {
    assert_trade_detail_of(world, &symbol, "sell", quantity, price_cents, fee_cents);
}
