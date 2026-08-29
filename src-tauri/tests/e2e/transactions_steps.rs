use std::collections::HashSet;

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::commands::transactions::{
    delete_transaction_internal, insert_transaction, list_transactions_internal,
    update_transaction_internal,
};
use tauri_app_lib::db::new_uuid;
use tauri_app_lib::error::AppError;
use tauri_app_lib::models::{TransactionInput, TransactionListFilter};
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

/// 按 id 全字段替换最近一笔交易（修改场景），沿用原交易账户/币种等非编辑字段。
#[when(expr = "修改最近交易 类型 {string} 金额 {int} 日期 {string} 备注 {string}")]
fn update_last_txn(world: &mut LedgerWorld, kind: String, amount: i64, date: String, note: String) {
    let id = world.last_transaction_id.clone().expect("没有可修改的交易");
    let existing = world
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在");
    let input = TransactionInput {
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: existing.currency_code.clone(),
        account_id: existing.account_id.clone(),
        to_account_id: existing.to_account_id.clone(),
        category_id: existing.category_id.clone(),
        merchant_id: existing.merchant_id.clone(),
        refund_of_transaction_id: existing.refund_of_transaction_id.clone(),
        note: Some(note),
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = update_transaction_internal(&world.conn, &id, input);
    assert!(result.is_ok(), "修改交易失败: {:?}", result.err());
    world.transactions_list = query_all_transactions(&world.conn);
}

/// 尝试把最近一笔交易改为转账（缺目标账户），应触发按 kind 校验并记录错误。
#[when(expr = "尝试修改最近交易为转账 金额 {int} 日期 {string}")]
fn try_update_last_to_transfer(world: &mut LedgerWorld, amount: i64, date: String) {
    let id = world.last_transaction_id.clone().expect("没有可修改的交易");
    let existing = world
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在");
    let input = TransactionInput {
        kind: TransactionKind::Transfer,
        amount_cents: amount,
        currency_code: existing.currency_code.clone(),
        account_id: existing.account_id.clone(),
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
    world.last_error = match update_transaction_internal(&world.conn, &id, input) {
        Err(AppError::Invalid(msg)) => Some(msg),
        _ => Some("预期失败但成功了".into()),
    };
}

/// 尝试修改一笔不存在的交易，应返回明确错误（NotFound）。
#[when(expr = "尝试修改不存在的交易 金额 {int} 日期 {string}")]
fn try_update_missing_txn(world: &mut LedgerWorld, amount: i64, date: String) {
    let input = TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: "missing-acc".into(),
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
    world.last_error = match update_transaction_internal(&world.conn, "nonexistent-id", input) {
        Err(AppError::NotFound(msg)) => Some(msg),
        _ => Some("预期失败但成功了".into()),
    };
}

/// 删除最近一笔交易（软删除，与 IPC/HTTP 删除同一行为层权威），
/// 供「编辑已删除交易」场景铺垫。
#[when(expr = "删除最近交易")]
fn delete_last_txn(world: &mut LedgerWorld) {
    let id = world.last_transaction_id.clone().expect("没有可删除的交易");
    delete_transaction_internal(&world.conn, &id).expect("删除交易失败");
    world.transactions_list = query_all_transactions(&world.conn);
}

/// 查询账户币种（买入/卖出以账户币种成交，与真实写路径一致）。
fn account_currency(conn: &rusqlite::Connection, account_id: &str) -> String {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1",
        params![account_id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 按标的 + 动作（buy/sell）定位交易 id：场景内同标的多笔买卖并存，
/// 不能依赖「最近交易」指针（买入后再卖出，最近交易已指向卖出）。
fn trade_txn_id(world: &LedgerWorld, symbol: &str, action: &str) -> String {
    world
        .conn
        .query_row(
            "SELECT st.transaction_id FROM security_transactions st \
             JOIN instruments i ON i.id = st.instrument_id \
             WHERE i.symbol=?1 AND st.action=?2",
            params![symbol, action],
            |r| r.get(0),
        )
        .expect("未找到对应买卖交易")
}

/// 经行为层创建一笔 buy/sell 交易（issue #180 编辑场景铺垫）：走真实写路径
/// plan → insert → apply（买入建仓 / 卖出 FIFO 匹配），并记录为最近交易。
fn insert_trade_for_edit(
    world: &mut LedgerWorld,
    kind: TransactionKind,
    symbol: &str,
    quantity: i64,
    price_cents: i64,
    account_name: &str,
    date: &str,
) {
    let instrument_id: String = world
        .conn
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .expect("标的不存在，先铺垫 Given 存在标的");
    let account_id = world.account_id(account_name);
    let currency_code = account_currency(&world.conn, &account_id);
    let input = TransactionInput {
        kind,
        amount_cents: quantity * price_cents,
        currency_code,
        account_id,
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id),
        quantity: Some(quantity as f64),
        price_cents: Some(price_cents),
        fee_cents: Some(0),
        idempotency_key: None,
    };
    let id = insert_transaction(&world.conn, input).expect("创建买卖交易失败");
    world.last_transaction_id = Some(id);
    world.transactions_list = query_all_transactions(&world.conn);
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

/// 构造 buy/sell 编辑入参（issue #180）：instrument_id 取自 security_transactions
/// （扩展表投影，与前端编辑回填同一数据源）；金额传 0（后端按数量×单价±手续费重算）。
fn trade_edit_input(
    world: &LedgerWorld,
    kind: TransactionKind,
    id: &str,
    quantity: i64,
    price_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    let instrument_id: String = world
        .conn
        .query_row(
            "SELECT instrument_id FROM security_transactions WHERE transaction_id=?1",
            params![id],
            |r| r.get(0),
        )
        .expect("该交易无买卖明细");
    let existing = world
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原交易不存在");
    TransactionInput {
        kind,
        amount_cents: 0,
        currency_code: existing.currency_code.clone(),
        account_id: existing.account_id.clone(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: existing.note.clone(),
        date: existing.date.clone(),
        instrument_id: Some(instrument_id),
        quantity: Some(quantity as f64),
        price_cents: Some(price_cents),
        fee_cents: Some(fee_cents),
        idempotency_key: None,
    }
}

/// 修改买入交易（issue #180）：全字段替换后重建持仓批次。
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
    let result = update_transaction_internal(&world.conn, &id, input);
    assert!(result.is_ok(), "修改买入交易失败: {:?}", result.err());
    world.transactions_list = query_all_transactions(&world.conn);
}

/// 尝试修改买入交易，应触发部分卖出守卫并记录错误（issue #180）。
#[when(expr = "尝试修改买入交易 {string} 数量 {int} 单价 {int}")]
fn try_update_partially_sold_buy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
) {
    let id = trade_txn_id(world, &symbol, "buy");
    let input = trade_edit_input(world, TransactionKind::Buy, &id, quantity, price_cents, 0);
    world.last_error = match update_transaction_internal(&world.conn, &id, input) {
        Err(AppError::Invalid(msg)) => Some(msg),
        _ => Some("预期失败但成功了".into()),
    };
}

/// 修改卖出交易（issue #180）：回补持仓后按新输入重建卖出匹配。
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
    let result = update_transaction_internal(&world.conn, &id, input);
    assert!(result.is_ok(), "修改卖出交易失败: {:?}", result.err());
    world.transactions_list = query_all_transactions(&world.conn);
}

/// 尝试修改一笔已删除的交易，应返回明确错误（NotFound：已删除与不存在同口径）。
#[when(expr = "尝试修改已删除的交易 金额 {int} 日期 {string}")]
fn try_update_deleted_txn(world: &mut LedgerWorld, amount: i64, date: String) {
    let id = world.last_transaction_id.clone().expect("没有可修改的交易");
    let input = TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: "acc-x".into(),
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
    world.last_error = match update_transaction_internal(&world.conn, &id, input) {
        Err(AppError::NotFound(msg)) => Some(msg),
        _ => Some("预期失败但成功了".into()),
    };
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

#[then(expr = "第 {int} 条交易版本应为 {int}")]
fn check_txn_version(world: &mut LedgerWorld, index: i64, expected_version: i64) {
    let idx = (index - 1) as usize;
    assert!(
        idx < world.transactions_list.len(),
        "交易列表只有 {} 条，无法访问第 {index} 条",
        world.transactions_list.len()
    );
    assert_eq!(
        world.transactions_list[idx].version, expected_version,
        "交易版本号不匹配"
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

// ---------------------------------------------------------------------------
// 服务端分页（issue-32：items + total，offset 页码模式）
// ---------------------------------------------------------------------------

/// 同日批量导入：直接写库模拟"同一批导入每批一个时间戳"（全部行 created_at 相同，
/// 只有 id tiebreaker 能区分排序），用于验证确定性排序下翻页无重复无遗漏。
#[when(expr = "批量导入 {int} 笔同日交易 日期 {string} 到账户 {string}")]
fn batch_import_same_day(world: &mut LedgerWorld, count: i64, date: String, account_name: String) {
    let account_id = world.account_id(&account_name);
    for i in 0..count {
        let id = new_uuid();
        let amount = (i + 1) * 100;
        world
            .conn
            .execute(
                "INSERT INTO transactions \
                 (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
                 category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
                 VALUES (?1,'expense',?2,'CNY',?2,?3,NULL,NULL,NULL,NULL,?4,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
                params![id, amount, account_id, date],
            )
            .unwrap();
    }
    world.transactions_list = query_all_transactions(&world.conn);
}

/// 执行分页查询并断言当前页条数与 total，快照 items 供后续步骤使用。
/// 四个 `check_page*` step 共用（避免重复的断言形状）。
fn assert_paged(
    world: &mut LedgerWorld,
    filter: TransactionListFilter,
    expected_count: i64,
    expected_total: i64,
    label: &str,
) {
    let result = list_transactions_internal(&world.conn, &filter).expect("分页查询失败");
    assert_eq!(
        result.items.len() as i64,
        expected_count,
        "{label} 返回条数不匹配"
    );
    assert_eq!(result.total, expected_total, "{label} total 不匹配");
    world.transactions_list = result.items;
}

#[then(expr = "分页查询 page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page(
    world: &mut LedgerWorld,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    assert_paged(
        world,
        TransactionListFilter {
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("page={page} page_size={page_size}"),
    );
}

#[then(expr = "分页查询 账户 {string} page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_account(
    world: &mut LedgerWorld,
    account_name: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let account_id = world.account_id(&account_name);
    assert_paged(
        world,
        TransactionListFilter {
            account_id: Some(account_id),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("账户 '{account_name}' page={page}"),
    );
}

#[then(expr = "分页查询 涉及账户 {string} page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_involving_account(
    world: &mut LedgerWorld,
    account_name: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    let account_id = world.account_id(&account_name);
    assert_paged(
        world,
        TransactionListFilter {
            involving_account_id: Some(account_id),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("涉及账户 '{account_name}' page={page}"),
    );
}

#[then(expr = "分页查询 kind {string} page {int} page_size {int} 应返回 {int} 条 total {int}")]
fn check_page_kind(
    world: &mut LedgerWorld,
    kind: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    assert_paged(
        world,
        TransactionListFilter {
            kind: Some(
                TransactionKind::parse(&kind)
                    .unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
            ),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("kind 过滤后 page={page}"),
    );
}

#[then(
    expr = "分页查询 日期 {string} 至 {string} page {int} page_size {int} 应返回 {int} 条 total {int}"
)]
fn check_page_date(
    world: &mut LedgerWorld,
    from: String,
    to: String,
    page: i64,
    page_size: i64,
    expected_count: i64,
    expected_total: i64,
) {
    assert_paged(
        world,
        TransactionListFilter {
            from: Some(from.clone()),
            to: Some(to.clone()),
            page: Some(page as usize),
            page_size: Some(page_size as usize),
            ..Default::default()
        },
        expected_count,
        expected_total,
        &format!("日期区间 [{from}, {to}] page={page}"),
    );
}

#[then(expr = "缺省查询 应返回 {int} 条 total {int}")]
fn check_default(world: &mut LedgerWorld, expected_count: i64, expected_total: i64) {
    let result = list_transactions_internal(&world.conn, &TransactionListFilter::default())
        .expect("缺省查询失败");
    assert_eq!(
        result.items.len() as i64,
        expected_count,
        "缺省查询应返回全部"
    );
    assert_eq!(result.total, expected_total, "缺省查询 total 不匹配");
    world.transactions_list = result.items;
}

#[then(expr = "读取 limit {int} 应返回 {int} 条")]
fn check_limit(world: &mut LedgerWorld, limit: i64, expected: i64) {
    let result = list_transactions_internal(
        &world.conn,
        &TransactionListFilter {
            limit: Some(limit),
            ..Default::default()
        },
    )
    .expect("limit 查询失败");
    assert_eq!(
        result.items.len() as i64,
        expected,
        "limit={limit} 返回条数不匹配"
    );
}

#[then(expr = "翻页 page_size {int} 应覆盖全部 {int} 条无重复无遗漏")]
fn check_pages_cover_all(world: &mut LedgerWorld, page_size: i64, expected_total: i64) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut page: usize = 1;
    loop {
        let result = list_transactions_internal(
            &world.conn,
            &TransactionListFilter {
                page: Some(page),
                page_size: Some(page_size as usize),
                ..Default::default()
            },
        )
        .expect("翻页查询失败");
        assert_eq!(result.total, expected_total, "total 应保持过滤后总数");
        for t in &result.items {
            assert!(seen.insert(t.id.clone()), "翻页出现重复交易: {}", t.id);
        }
        if (result.items.len() as i64) < page_size {
            break;
        }
        page += 1;
    }
    assert_eq!(
        seen.len() as i64,
        expected_total,
        "翻页覆盖条数不匹配（有遗漏）"
    );
}

#[then(expr = "标的 {string} 持仓数量应为 {int}")]
fn assert_holding_quantity(world: &mut LedgerWorld, symbol: String, expected: i64) {
    // 按标的定位持仓批次（场景内单账户，标的唯一确定批次）
    let quantity: f64 = world
        .conn
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

/// 断言买入/卖出明细与预期一致（编辑回填数据源：security_transactions JOIN instruments）。
fn assert_trade_detail_of(
    world: &LedgerWorld,
    symbol: &str,
    action: &str,
    quantity: i64,
    price_cents: i64,
    fee_cents: i64,
) {
    let id = trade_txn_id(world, symbol, action);
    let expected_instrument_id: String = world
        .conn
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .expect("标的不存在");
    // 直接断言扩展表投影（与 IPC get_transaction_trade 同一数据源：
    // security_transactions JOIN instruments），验证编辑后回填数据正确
    let (instrument_id, trade_quantity, trade_price, trade_fee): (String, f64, i64, i64) = world
        .conn
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
