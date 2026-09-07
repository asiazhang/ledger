//! 交易写入 BDD 步骤（issue #761 迁移）：交易输入构造收编 L1 步骤输入工厂
//! （[`crate::step_inputs`]）、写入收编 L2 步骤动词（[`crate::step_verbs`]）——
//! 步骤函数薄化为文本解析 + 快照刷新；错误断言路径改走动词 try 形态（构造 +
//! 显式写入捕获），断言语义不变。「存在账户」前置经账户域公开创建入口动词
//! （#763 旁路归零）；注入触发器两步骤为纯测试侧注入（spec #169 定案），留直连例外。

use cucumber::{given, then, when};

use tauri_app_lib::transaction::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::{capture_expected_error, instrument_id_by_symbol, query_all_transactions};
use crate::step_inputs::{buy_input, parse_kind, plain_input, trade_input};
use crate::step_verbs;
use crate::step_verbs::{
    create_account_verb, create_transaction_verb, refund_last_transaction,
    try_create_transaction_verb, try_delete_transaction_verb,
};
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "存在账户 {string} 类型 {string} 币种 {string}")]
fn create_account(world: &mut LedgerWorld, name: String, kind: String, currency: String) {
    create_account_verb(world, &name, &kind, &currency, None);
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
    // 缺转入账户：被测前提是 writer 守卫 `transfer.to-account-required`，
    // 经通用底座构造不合法形态、try 动词显式捕获（不静默吞错）。
    let input = plain_input(TransactionKind::Transfer, amount, &account_id, &date);
    let result = try_create_transaction_verb(world, input);
    capture_expected_error(world, result);
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
    let account_id = world.account_id(&account_name);
    let input = plain_input(parse_kind(&kind), amount, &account_id, &date);
    let result = try_create_transaction_verb(world, input);
    capture_expected_error(world, result);
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

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

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
    let txn = world.txn.transactions_list.last().expect("交易列表为空");
    assert_eq!(txn.kind.as_str(), expected_kind);
}

#[then(expr = "该转账 account_id 应匹配账户 {string}")]
fn check_transfer_from(world: &mut LedgerWorld, account_name: String) {
    let txn = world.txn.transactions_list.last().expect("交易列表为空");
    let expected_id = world.account_id(&account_name);
    assert_eq!(txn.account_id, expected_id);
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
    // 第一条是原支出（date DESC 排序，后创建的 refund 排前面）
    // 实际上：expense 日期 04-01, refund 日期 04-05
    // 按 date DESC: refund (04-05) 在前，expense (04-01) 在后
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
