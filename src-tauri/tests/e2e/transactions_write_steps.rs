use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::error::AppError;
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::{create_transaction_internal, delete_transaction_internal};

use crate::common::{insert_account, new_account_id, query_all_transactions};
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "存在账户 {string} 类型 {string} 币种 {string}")]
fn create_account(world: &mut LedgerWorld, name: String, kind: String, currency: String) {
    let id = new_account_id();
    insert_account(&world_conn!(world), &id, &name, &kind, &currency);
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
        merchant_name: None,
        policy_id: None,
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
    // 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）创建，提交点置脏/到期检查。
    let result = world
        .db
        .write(|conn| create_transaction_internal(conn, input));
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap().id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
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
        merchant_name: None,
        policy_id: None,
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
    // 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）创建，提交点置脏/到期检查。
    let result = world
        .db
        .write(|conn| create_transaction_internal(conn, input));
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap().id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "尝试创建转账 金额 {int} 从账户 {string} 日期 {string}")]
fn try_transfer_without_target(
    world: &mut LedgerWorld,
    amount: i64,
    account_name: String,
    date: String,
) {
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
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
    let result = create_transaction_internal(&world_conn!(world), input);
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
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
        merchant_name: None,
        policy_id: None,
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
    let result = create_transaction_internal(&world_conn!(world), input);
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        _ => Some("预期失败但成功了".into()),
    };
}

/// 尝试创建一笔买入交易并捕获错误（供「应返回错误」断言，issue #228）。
#[when(expr = "尝试买入标的 {string} 数量 {int} 单价 {int} 到投资账户 {string}")]
fn try_create_buy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    account_name: String,
) {
    let instrument_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .expect("标的不存在，先铺垫 Given 存在标的");
    let account_id = world.account_id(&account_name);
    let currency_code: String = world_conn!(world)
        .query_row(
            "SELECT currency_code FROM accounts WHERE id=?1",
            params![account_id],
            |r| r.get(0),
        )
        .expect("账户不存在");
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code,
        account_id,
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id),
        quantity: Some(quantity as f64),
        price_cents: Some(price_cents),
        fee_cents: Some(0),
        idempotency_key: None,
    };
    world.last_error = match create_transaction_internal(&world_conn!(world), input) {
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    };
}

/// 尝试创建一笔买入/卖出交易并捕获错误，标的按裸 id 提交（不查字典）——
/// 供「引用不存在标的」场景使用（issue #295，prepare 标的存在性校验）。
fn try_create_trade_with_raw_instrument_id(
    world: &mut LedgerWorld,
    kind: TransactionKind,
    instrument_id: &str,
    quantity: i64,
    price_cents: i64,
    account_name: &str,
) {
    let account_id = world.account_id(account_name);
    let currency_code: String = world_conn!(world)
        .query_row(
            "SELECT currency_code FROM accounts WHERE id=?1",
            params![account_id],
            |r| r.get(0),
        )
        .expect("账户不存在");
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind,
        amount_cents: 0,
        currency_code,
        account_id,
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.to_string()),
        quantity: Some(quantity as f64),
        price_cents: Some(price_cents),
        fee_cents: Some(0),
        idempotency_key: None,
    };
    world.last_error = match create_transaction_internal(&world_conn!(world), input) {
        Ok(_) => Some("预期失败但成功了".into()),
        Err(e) => Some(e.to_string()),
    };
}

/// 尝试买入不存在的标的并捕获错误（裸 id 直提，供「应返回错误」断言，issue #295）。
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

/// 尝试卖出不存在的标的并捕获错误（裸 id 直提，供「应返回错误」断言，issue #295）。
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

/// 注入「建仓中途失败」：买入副作用先写 security_transactions、再写 security_lots，
/// 触发器在第二步 RAISE(ABORT)——纯测试侧注入（spec #169 定案），检验 create
/// 编排入口把行落库与半套副作用整体回滚（issue #228）。
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

/// 注入「软删中途失败」：删除买入时行为层 revert（清理持仓批次）先成功、
/// 软删 UPDATE 被触发器 RAISE(ABORT) 挡下——纯测试侧注入（spec #169 定案），
/// 检验 delete 编排入口把持仓清理与软删纳入同一事务、中途失败整体回滚（issue #229）。
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

/// 尝试删除最近一笔交易并捕获错误（供「应返回错误」断言，issue #229）。
#[when(expr = "尝试删除最近交易")]
fn try_delete_last_txn(world: &mut LedgerWorld) {
    let id = world.last_transaction_id.clone().expect("没有可删除的交易");
    world.last_error = match delete_transaction_internal(&world_conn!(world), &id) {
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
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
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
    let result = create_transaction_internal(&world_conn!(world), input);
    assert!(result.is_ok(), "创建转账失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap().id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "关联上一笔交易创建退款 金额 {int} 日期 {string}")]
fn create_refund(world: &mut LedgerWorld, amount: i64, date: String) {
    let expense_id = world
        .last_transaction_id
        .clone()
        .expect("没有上一笔交易可关联");
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
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
    let result = create_transaction_internal(&world_conn!(world), input);
    assert!(result.is_ok(), "创建退款失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap().id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "交易列表应包含 {int} 条记录")]
fn check_transaction_count(world: &mut LedgerWorld, expected: i64) {
    world.transactions_list = query_all_transactions(&world_conn!(world));
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

/// 建仓中途失败整体回滚的终态断言：持仓批次与买卖明细均无残留
/// （交易行无残留由「交易列表应包含 0 条记录」断言，issue #228）。
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
