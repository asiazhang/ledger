//! 场外基金申赎记账 BDD 步骤（issue #302 / ADR-0038 金额权威）：确认单整分金额 +
//! 确认份额为权威输入（wire 不携带单价，由后端行为层反算净值），持仓与已实现
//! 盈亏读回断言（闭合不变式：全平仓 Σ 已实现盈亏 = Σ 卖出金额 − Σ 买入金额）。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::commands::transactions::create_transaction_internal;
use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::query_all_transactions;
use crate::world::LedgerWorld;

#[given(expr = "存在基金标的 {string} 名称 {string}")]
fn create_fund_instrument(world: &mut LedgerWorld, symbol: String, name: String) {
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'fund',?3,'CNY','unknown',?4,?4,1,?5)",
            params![new_uuid(), symbol, name, now, device_id()],
        )
        .unwrap();
}

/// 按确认单录入基金申赎（金额权威：amount_cents = 确认单整分金额，wire 不带单价）。
fn fund_trade(
    world: &mut LedgerWorld,
    kind: TransactionKind,
    symbol: &str,
    quantity: f64,
    amount_cents: i64,
    fee_cents: i64,
    account_name: &str,
) {
    // 申购/赎回错开交易日（与持仓批次 FIFO 排序、列表断言的日期序一致）
    let date = match kind {
        TransactionKind::Buy => "2026-01-10",
        _ => "2026-01-20",
    };
    let instrument_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .expect("基金标的不存在，先铺垫 Given 存在基金标的");
    let account_id = world.account_id(account_name);
    let currency_code = world_conn!(world)
        .query_row(
            "SELECT currency_code FROM accounts WHERE id=?1",
            params![account_id],
            |r| r.get(0),
        )
        .expect("账户不存在");
    let input = TransactionInput {
        merchant_name: None,
        kind,
        amount_cents,
        currency_code,
        account_id,
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id),
        quantity: Some(quantity),
        // 金额权威：单价不落 wire，由后端按（金额 ∓ 手续费）÷ 份额反算（issue #302）
        price_cents: None,
        fee_cents: Some(fee_cents),
        idempotency_key: None,
    };
    let write = create_transaction_internal(&world_conn!(world), input).expect("基金申赎落库失败");
    world.last_transaction_id = Some(write.id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "按确认单申购基金 {string} 份额 {float} 金额 {int} 手续费 {int} 到投资账户 {string}")]
fn fund_buy(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: f64,
    amount_cents: i64,
    fee_cents: i64,
    account_name: String,
) {
    fund_trade(
        world,
        TransactionKind::Buy,
        &symbol,
        quantity,
        amount_cents,
        fee_cents,
        &account_name,
    );
}

#[when(expr = "按确认单赎回基金 {string} 份额 {float} 金额 {int} 手续费 {int} 从投资账户 {string}")]
fn fund_sell(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: f64,
    amount_cents: i64,
    fee_cents: i64,
    account_name: String,
) {
    fund_trade(
        world,
        TransactionKind::Sell,
        &symbol,
        quantity,
        amount_cents,
        fee_cents,
        &account_name,
    );
}

/// 断言最近一笔申赎的明细（扩展表投影：份额/反算净值/手续费）。
fn assert_fund_trade_detail(
    world: &LedgerWorld,
    symbol: &str,
    quantity: f64,
    nav_price_cents: i64,
    fee_cents: i64,
) {
    let id = world
        .last_transaction_id
        .clone()
        .expect("没有最近的申赎交易");
    let expected_instrument_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .expect("标的不存在");
    let (instrument_id, trade_quantity, trade_price, trade_fee): (String, f64, i64, i64) =
        world_conn!(world)
            .query_row(
                "SELECT st.instrument_id, st.quantity, st.price_cents, st.fee_cents \
                 FROM security_transactions st WHERE st.transaction_id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("读取买卖明细失败");
    assert_eq!(instrument_id, expected_instrument_id, "标的不符");
    assert!(
        (trade_quantity - quantity).abs() < 1e-9,
        "份额不符: 期望 {quantity}，实际 {trade_quantity}"
    );
    assert_eq!(
        trade_price, nav_price_cents,
        "反算净值不符（万分之一元刻度）"
    );
    assert_eq!(trade_fee, fee_cents, "手续费不符");
}

#[then(expr = "该买入明细应为 标的 {string} 份额 {float} 净值 {int} 手续费 {int}")]
fn assert_fund_buy_detail(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: f64,
    nav_price_cents: i64,
    fee_cents: i64,
) {
    assert_fund_trade_detail(world, &symbol, quantity, nav_price_cents, fee_cents);
}

#[then(expr = "该卖出明细应为 标的 {string} 份额 {float} 净值 {int} 手续费 {int}")]
fn assert_fund_sell_detail(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: f64,
    nav_price_cents: i64,
    fee_cents: i64,
) {
    assert_fund_trade_detail(world, &symbol, quantity, nav_price_cents, fee_cents);
}

#[then(expr = "标的 {string} 持仓份额应为 {float}")]
fn assert_fund_holding_quantity(world: &mut LedgerWorld, symbol: String, expected: f64) {
    let quantity: f64 = world_conn!(world)
        .query_row(
            "SELECT remaining_quantity FROM security_lots \
             WHERE instrument_id = (SELECT id FROM instruments WHERE symbol=?1)",
            params![symbol],
            |r| r.get(0),
        )
        .expect("该标的的持仓批次不存在");
    assert!(
        (quantity - expected).abs() < 1e-6,
        "持仓份额不符: 期望 {expected}，实际 {quantity}"
    );
}

#[then(expr = "基金 {string} 已实现盈亏合计应为 {int}")]
fn assert_fund_realized_pnl_total(world: &mut LedgerWorld, symbol: String, expected: i64) {
    let total: i64 = world_conn!(world)
        .query_row(
            "SELECT COALESCE(SUM(sls.realized_pnl_cents),0) FROM security_lot_sales sls \
             JOIN security_lots l ON l.id = sls.lot_id \
             WHERE l.instrument_id = (SELECT id FROM instruments WHERE symbol=?1)",
            params![symbol],
            |r| r.get(0),
        )
        .expect("读取已实现盈亏失败");
    assert_eq!(
        total, expected,
        "已实现盈亏合计不符（闭合不变式：应等于 Σ 卖出金额 − Σ 买入金额）"
    );
}
