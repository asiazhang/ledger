use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::error::AppError;
use tauri_app_lib::transaction::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::{
    create_transaction_internal, delete_transaction_internal, update_transaction_internal,
};

use crate::common::query_all_transactions;
use crate::world::LedgerWorld;

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
        merchant_name: None,
        policy_id: None,
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
    let result = update_transaction_internal(&world_conn!(world), &id, input);
    assert!(result.is_ok(), "修改交易失败: {:?}", result.err());
    world.transactions_list = query_all_transactions(&world_conn!(world));
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
        merchant_name: None,
        policy_id: None,
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
    world.last_error = match update_transaction_internal(&world_conn!(world), &id, input) {
        Err(AppError::Coded { message, .. }) => Some(message),
        _ => Some("预期失败但成功了".into()),
    };
}

/// 尝试修改一笔不存在的交易，应返回明确错误（NotFound）。
#[when(expr = "尝试修改不存在的交易 金额 {int} 日期 {string}")]
fn try_update_missing_txn(world: &mut LedgerWorld, amount: i64, date: String) {
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
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
    world.last_error =
        match update_transaction_internal(&world_conn!(world), "nonexistent-id", input) {
            Err(AppError::Coded { message, .. }) => Some(message),
            _ => Some("预期失败但成功了".into()),
        };
}

/// 删除最近一笔交易（软删除，与 IPC/HTTP 删除同一行为层权威），
/// 供「编辑已删除交易」场景铺垫。
#[when(expr = "删除最近交易")]
fn delete_last_txn(world: &mut LedgerWorld) {
    let id = world.last_transaction_id.clone().expect("没有可删除的交易");
    delete_transaction_internal(&world_conn!(world), &id).expect("删除交易失败");
    world.transactions_list = query_all_transactions(&world_conn!(world));
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
    let instrument_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .expect("标的不存在，先铺垫 Given 存在标的");
    let account_id = world.account_id(account_name);
    let currency_code = account_currency(&world_conn!(world), &account_id);
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
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
    let write = create_transaction_internal(&world_conn!(world), input).expect("创建买卖交易失败");
    world.last_transaction_id = Some(write.id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
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
    let instrument_id: String = world_conn!(world)
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
        merchant_name: None,
        policy_id: None,
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
    let result = update_transaction_internal(&world_conn!(world), &id, input);
    assert!(result.is_ok(), "修改买入交易失败: {:?}", result.err());
    world.transactions_list = query_all_transactions(&world_conn!(world));
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
    world.last_error = match update_transaction_internal(&world_conn!(world), &id, input) {
        Err(AppError::Coded { message, .. }) => Some(message),
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
    let result = update_transaction_internal(&world_conn!(world), &id, input);
    assert!(result.is_ok(), "修改卖出交易失败: {:?}", result.err());
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

/// 尝试修改一笔已删除的交易，应返回明确错误（NotFound：已删除与不存在同口径）。
#[when(expr = "尝试修改已删除的交易 金额 {int} 日期 {string}")]
fn try_update_deleted_txn(world: &mut LedgerWorld, amount: i64, date: String) {
    let id = world.last_transaction_id.clone().expect("没有可修改的交易");
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
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
    world.last_error = match update_transaction_internal(&world_conn!(world), &id, input) {
        Err(AppError::Coded { message, .. }) => Some(message),
        _ => Some("预期失败但成功了".into()),
    };
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

#[then(expr = "标的 {string} 持仓数量应为 {int}")]
fn assert_holding_quantity(world: &mut LedgerWorld, symbol: String, expected: i64) {
    // 按标的定位持仓批次（场景内单账户，标的唯一确定批次）
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
    let expected_instrument_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .expect("标的不存在");
    // 直接断言扩展表投影（与 IPC get_transaction_trade 同一数据源：
    // security_transactions JOIN instruments），验证编辑后回填数据正确
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
