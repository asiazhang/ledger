use cucumber::{given, then, when};

use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;

use crate::common::{capture_expected_error, instrument_id_by_symbol, query_all_transactions};
use crate::step_inputs::{buy_input, parse_kind, plain_input, trade_input};
use crate::step_verbs;
use crate::step_verbs::{
    create_account_verb, create_transaction_verb, refund_last_transaction,
    try_create_transaction_verb, try_delete_transaction_verb,
};
use crate::world::LedgerWorld;

#[given(expr = "存在账户 {string} 类型 {string} 币种 {string}")]
fn create_account(world: &mut LedgerWorld, name: String, kind: String, currency: String) {
    create_account_verb(world, &name, &kind, &currency, None);
}

#[when(expr = "创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string}")]
fn create_txn(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
) {
    let account_id = world.account_id(&account_name);
    create_transaction_verb(
        world,
        plain_input(parse_kind(&kind), amount, &account_id, &date),
    );
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string} 备注 {string}")]
fn create_txn_with_note(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
    note: String,
) {
    let account_id = world.account_id(&account_name);
    create_transaction_verb(
        world,
        TransactionInput {
            note: Some(note),
            ..plain_input(parse_kind(&kind), amount, &account_id, &date)
        },
    );
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "尝试创建转账 金额 {int} 从账户 {string} 日期 {string}")]
fn try_transfer_without_target(
    world: &mut LedgerWorld,
    amount: i64,
    account_name: String,
    date: String,
) {
    let account_id = world.account_id(&account_name);
    let input = plain_input(TransactionKind::Transfer, amount, &account_id, &date);
    let result = try_create_transaction_verb(world, input);
    capture_expected_error(world, result);
}

#[when(expr = "尝试创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string}")]
fn try_create_txn(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
) {
    let account_id = world.account_id(&account_name);
    let input = plain_input(parse_kind(&kind), amount, &account_id, &date);
    let result = try_create_transaction_verb(world, input);
    capture_expected_error(world, result);
}

#[when(expr = "尝试买入标的 {string} 数量 {int} 单价 {int} 到投资账户 {string}")]
fn try_create_buy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    account_name: String,
) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    let account_id = world.account_id(&account_name);
    let input = buy_input(
        &instrument_id,
        quantity as f64,
        Some(price_cents),
        &account_id,
        "2026-01-10",
    );
    world.last_error = match try_create_transaction_verb(world, input) {
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    };
}

fn try_create_trade_with_raw_instrument_id(
    world: &mut LedgerWorld,
    kind: TransactionKind,
    instrument_id: &str,
    quantity: i64,
    price_cents: i64,
    account_name: &str,
) {
    let account_id = world.account_id(account_name);
    let input = trade_input(
        kind,
        instrument_id,
        quantity as f64,
        price_cents,
        &account_id,
        "2026-01-10",
    );
    world.last_error = match try_create_transaction_verb(world, input) {
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    };
}

#[when(expr = "尝试买入不存在标的 {string} 数量 {int} 单价 {int} 到投资账户 {string}")]
fn try_create_buy_missing_instrument(
    world: &mut LedgerWorld,
    instrument_id: String,
    quantity: i64,
    price_cents: i64,
    account_name: String,
) {
    try_create_trade_with_raw_instrument_id(
        world,
        TransactionKind::Buy,
        &instrument_id,
        quantity,
        price_cents,
        &account_name,
    );
}

#[when(expr = "尝试卖出不存在标的 {string} 数量 {int} 单价 {int} 从投资账户 {string}")]
fn try_create_sell_missing_instrument(
    world: &mut LedgerWorld,
    instrument_id: String,
    quantity: i64,
    price_cents: i64,
    account_name: String,
) {
    try_create_trade_with_raw_instrument_id(
        world,
        TransactionKind::Sell,
        &instrument_id,
        quantity,
        price_cents,
        &account_name,
    );
}

#[when(expr = "注入买入建仓中途失败触发器")]
fn inject_buy_lot_failure_trigger(world: &mut LedgerWorld) {
    world_conn!(world)
        .execute(
            "CREATE TRIGGER block_buy_lot BEFORE INSERT ON security_lots \
             BEGIN SELECT RAISE(ABORT, '测试注入：建仓失败'); END",
            [],
        )
        .expect("注入触发器失败");
}

#[when(expr = "注入软删失败触发器")]
fn inject_soft_delete_failure_trigger(world: &mut LedgerWorld) {
    world_conn!(world)
        .execute(
            "CREATE TRIGGER block_soft_delete BEFORE UPDATE ON transactions \
             BEGIN SELECT RAISE(ABORT, '测试注入：软删失败'); END",
            [],
        )
        .expect("注入触发器失败");
}

#[when(expr = "尝试删除最近交易")]
fn try_delete_last_txn(world: &mut LedgerWorld) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可删除的交易");
    world.last_error = match try_delete_transaction_verb(world, &id) {
        Ok(()) => None,
        Err(e) => Some(e.to_string()),
    };
}

#[when(expr = "创建转账 金额 {int} 从 {string} 到 {string} 日期 {string}")]
fn create_transfer(
    world: &mut LedgerWorld,
    amount: i64,
    from_name: String,
    to_name: String,
    date: String,
) {
    step_verbs::create_transfer(world, amount, &from_name, &to_name, &date);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "关联上一笔交易创建退款 金额 {int} 日期 {string}")]
fn create_refund(world: &mut LedgerWorld, amount: i64, date: String) {
    refund_last_transaction(world, amount, &date);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[then(expr = "交易列表应包含 {int} 条记录")]
fn check_transaction_count(world: &mut LedgerWorld, expected: i64) {
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
    assert_eq!(
        world.txn.transactions_list.len() as i64,
        expected,
        "交易数量不匹配"
    );
}

#[then(expr = "第 {int} 条交易类型应为 {string} 金额应为 {int}")]
fn check_txn_kind_amount(
    world: &mut LedgerWorld,
    index: i64,
    expected_kind: String,
    expected_amount: i64,
) {
    let idx = (index - 1) as usize;
    assert!(
        idx < world.txn.transactions_list.len(),
        "交易列表只有 {} 条，无法访问第 {index} 条",
        world.txn.transactions_list.len()
    );
    let txn = &world.txn.transactions_list[idx];
    assert_eq!(txn.kind.as_str(), expected_kind, "交易类型不匹配");
    assert_eq!(txn.amount_cents, expected_amount, "交易金额不匹配");
}

#[then(expr = "第 {int} 条交易类型应为 {string} 金额应为 {int} 备注 {string}")]
fn check_txn_kind_amount_note(
    world: &mut LedgerWorld,
    index: i64,
    expected_kind: String,
    expected_amount: i64,
    expected_note: String,
) {
    let idx = (index - 1) as usize;
    assert!(
        idx < world.txn.transactions_list.len(),
        "交易列表只有 {} 条",
        world.txn.transactions_list.len()
    );
    let txn = &world.txn.transactions_list[idx];
    assert_eq!(txn.kind.as_str(), expected_kind, "交易类型不匹配");
    assert_eq!(txn.amount_cents, expected_amount, "交易金额不匹配");
    assert_eq!(
        txn.note.as_deref(),
        Some(expected_note.as_str()),
        "备注不匹配"
    );
}

#[then(expr = "应返回错误 {string}")]
fn check_error(world: &mut LedgerWorld, expected_msg: String) {
    crate::common::assert_last_error_contains(world, &expected_msg);
}

#[then(expr = "无买入持仓与买卖明细残留")]
fn assert_no_lot_and_trade_residue(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    let (lots, stx): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM security_lots), \
                    (SELECT COUNT(*) FROM security_transactions)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(lots, 0, "持仓批次不应残留");
    assert_eq!(stx, 0, "买卖明细不应残留");
}

#[then(expr = "该转账类型应为 {string}")]
fn check_transfer_kind(world: &mut LedgerWorld, expected_kind: String) {
    let txn = world.txn.transactions_list.last().expect("交易列表为空");
    assert_eq!(txn.kind.as_str(), expected_kind);
}

#[then(expr = "该转账 account_id 应匹配账户 {string}")]
fn check_transfer_from(world: &mut LedgerWorld, account_name: String) {
    let txn = world.txn.transactions_list.last().expect("交易列表为空");
    let expected_id = world.account_id(&account_name);
    assert_eq!(txn.account_id.as_deref(), Some(expected_id.as_str()));
}

#[then(expr = "该转账 to_account_id 应匹配账户 {string}")]
fn check_transfer_to(world: &mut LedgerWorld, account_name: String) {
    let txn = world.txn.transactions_list.last().expect("交易列表为空");
    let expected_id = world.account_id(&account_name);
    assert_eq!(txn.to_account_id.as_deref(), Some(expected_id.as_str()));
}

#[then(expr = "退款交易的 refund_of 应指向原支出交易")]
fn check_refund_linked(world: &mut LedgerWorld) {
    assert!(
        world.txn.transactions_list.len() >= 2,
        "需要有至少 2 条交易"
    );
    let refund = &world.txn.transactions_list[0];
    let expense = &world.txn.transactions_list[1];
    assert_eq!(refund.kind, TransactionKind::Refund, "第一条应为退款");
    assert_eq!(expense.kind, TransactionKind::Expense, "第二条应为原支出");
    assert_eq!(
        refund.refund_of_transaction_id.as_deref(),
        Some(expense.id.as_str()),
        "退款未正确关联原支出"
    );
}
