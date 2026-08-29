use cucumber::{given, then, when};

use tauri_app_lib::commands::transactions::insert_transaction;
use tauri_app_lib::error::AppError;
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::{insert_account, new_account_id, query_all_transactions};
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "存在账户 {string} 类型 {string} 币种 {string}")]
fn create_account(world: &mut LedgerWorld, name: String, kind: String, currency: String) {
    let id = new_account_id();
    insert_account(&world.conn, &id, &name, &kind, &currency);
    world.account_name_to_id.insert(name, id);
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string}")]
fn create_txn(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
) {
    let input = TransactionInput {
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: "CNY".into(),
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
    let result = insert_transaction(&world.conn, input);
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap());
    world.transactions_list = query_all_transactions(&world.conn);
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
    let input = TransactionInput {
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: Some(note),
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = insert_transaction(&world.conn, input);
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap());
    world.transactions_list = query_all_transactions(&world.conn);
}

#[when(expr = "尝试创建转账 金额 {int} 从账户 {string} 日期 {string}")]
fn try_transfer_without_target(
    world: &mut LedgerWorld,
    amount: i64,
    account_name: String,
    date: String,
) {
    let input = TransactionInput {
        kind: TransactionKind::Transfer,
        amount_cents: amount,
        currency_code: "CNY".into(),
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
    let result = insert_transaction(&world.conn, input);
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        _ => Some("预期失败但成功了".into()),
    };
}

/// 尝试创建一笔交易并捕获错误（供「应返回错误」断言）。
/// 与 `create_txn` 的区别：不要求成功，失败信息记入 `world.last_error`。
#[when(expr = "尝试创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string}")]
fn try_create_txn(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
) {
    let input = TransactionInput {
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: "CNY".into(),
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
    let result = insert_transaction(&world.conn, input);
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        _ => Some("预期失败但成功了".into()),
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
    let input = TransactionInput {
        kind: TransactionKind::Transfer,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(&from_name),
        to_account_id: Some(world.account_id(&to_name)),
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
    let result = insert_transaction(&world.conn, input);
    assert!(result.is_ok(), "创建转账失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap());
    world.transactions_list = query_all_transactions(&world.conn);
}

#[when(expr = "关联上一笔交易创建退款 金额 {int} 日期 {string}")]
fn create_refund(world: &mut LedgerWorld, amount: i64, date: String) {
    let expense_id = world
        .last_transaction_id
        .clone()
        .expect("没有上一笔交易可关联");
    let input = TransactionInput {
        kind: TransactionKind::Refund,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: {
            // 从已有交易中获取支出的 account_id
            let txn = world
                .transactions_list
                .iter()
                .find(|t| t.id == expense_id)
                .expect("原交易不存在");
            txn.account_id.clone()
        },
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: Some(expense_id),
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = insert_transaction(&world.conn, input);
    assert!(result.is_ok(), "创建退款失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap());
    world.transactions_list = query_all_transactions(&world.conn);
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "交易列表应包含 {int} 条记录")]
fn check_transaction_count(world: &mut LedgerWorld, expected: i64) {
    world.transactions_list = query_all_transactions(&world.conn);
    assert_eq!(
        world.transactions_list.len() as i64,
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
        idx < world.transactions_list.len(),
        "交易列表只有 {} 条，无法访问第 {index} 条",
        world.transactions_list.len()
    );
    let txn = &world.transactions_list[idx];
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
        idx < world.transactions_list.len(),
        "交易列表只有 {} 条",
        world.transactions_list.len()
    );
    let txn = &world.transactions_list[idx];
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

#[then(expr = "该转账类型应为 {string}")]
fn check_transfer_kind(world: &mut LedgerWorld, expected_kind: String) {
    let txn = world.transactions_list.last().expect("交易列表为空");
    assert_eq!(txn.kind.as_str(), expected_kind);
}

#[then(expr = "该转账 account_id 应匹配账户 {string}")]
fn check_transfer_from(world: &mut LedgerWorld, account_name: String) {
    let txn = world.transactions_list.last().expect("交易列表为空");
    let expected_id = world.account_id(&account_name);
    assert_eq!(txn.account_id, expected_id);
}

#[then(expr = "该转账 to_account_id 应匹配账户 {string}")]
fn check_transfer_to(world: &mut LedgerWorld, account_name: String) {
    let txn = world.transactions_list.last().expect("交易列表为空");
    let expected_id = world.account_id(&account_name);
    assert_eq!(txn.to_account_id.as_deref(), Some(expected_id.as_str()));
}

#[then(expr = "退款交易的 refund_of 应指向原支出交易")]
fn check_refund_linked(world: &mut LedgerWorld) {
    assert!(world.transactions_list.len() >= 2, "需要有至少 2 条交易");
    // 第一条是原支出（date DESC 排序，后创建的 refund 排前面）
    // 实际上：expense 日期 04-01, refund 日期 04-05
    // 按 date DESC: refund (04-05) 在前，expense (04-01) 在后
    let refund = &world.transactions_list[0];
    let expense = &world.transactions_list[1];
    assert_eq!(refund.kind, TransactionKind::Refund, "第一条应为退款");
    assert_eq!(expense.kind, TransactionKind::Expense, "第二条应为原支出");
    assert_eq!(
        refund.refund_of_transaction_id.as_deref(),
        Some(expense.id.as_str()),
        "退款未正确关联原支出"
    );
}
