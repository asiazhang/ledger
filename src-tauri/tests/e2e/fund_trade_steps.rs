use cucumber::{given, then, when};
use rusqlite::params;

use ledger_investment::{InstrumentInput, InstrumentType, create_instrument};
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;

use crate::common::{instrument_id_by_symbol, query_all_transactions};
use crate::step_inputs::{buy_input, sell_input};
use crate::world::LedgerWorld;

#[given(expr = "存在基金标的 {string} 名称 {string}")]
fn create_fund_instrument(world: &mut LedgerWorld, symbol: String, name: String) {
    let input = InstrumentInput {
        symbol: symbol.clone(),
        kind: InstrumentType::Fund,
        name: Some(name),
        currency_code: "CNY".into(),
        market: None,
    };
    world_write!(world, |conn| create_instrument(conn, input)).expect("新建基金标的失败");
}

struct FundConfirmation {
    quantity: f64,
    amount_cents: i64,
    fee_cents: i64,
}

fn fund_trade(
    world: &mut LedgerWorld,
    kind: TransactionKind,
    symbol: &str,
    confirmation: FundConfirmation,
    account_name: &str,
    funding_name: Option<&str>,
    date: &str,
) {
    let FundConfirmation {
        quantity,
        amount_cents,
        fee_cents,
    } = confirmation;
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), symbol);
    let account_id = world.account_id(account_name);
    let currency_code = world_conn!(world)
        .query_row(
            "SELECT currency_code FROM accounts WHERE id=?1",
            params![account_id],
            |r| r.get(0),
        )
        .expect("账户不存在");
    let base = match kind {
        TransactionKind::Buy => buy_input(&instrument_id, quantity, None, &account_id, date),
        TransactionKind::Sell => sell_input(&instrument_id, quantity, None, &account_id, date),
        other => panic!("基金申赎仅支持 buy/sell，收到: {other}"),
    };
    let input = TransactionInput {
        amount_cents,
        currency_code,
        fee_cents: Some(fee_cents),
        funding_account_id: funding_name.map(|n| world.account_id(n)),
        ..base
    };
    let write = create_transaction_internal(&world_conn!(world), input).expect("基金申赎落库失败");
    world.txn.last_transaction_id = Some(write.id);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
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
        FundConfirmation {
            quantity,
            amount_cents,
            fee_cents,
        },
        &account_name,
        None,
        "2026-01-10",
    );
}

#[when(
    expr = "按确认单于 {string} 申购基金 {string} 份额 {float} 金额 {int} 手续费 {int} 到投资账户 {string}"
)]
fn fund_buy_on(
    world: &mut LedgerWorld,
    date: String,
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
        FundConfirmation {
            quantity,
            amount_cents,
            fee_cents,
        },
        &account_name,
        None,
        &date,
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
        FundConfirmation {
            quantity,
            amount_cents,
            fee_cents,
        },
        &account_name,
        None,
        "2026-01-20",
    );
}

#[when(
    expr = "按确认单于 {string} 赎回基金 {string} 份额 {float} 金额 {int} 手续费 {int} 从投资账户 {string}"
)]
fn fund_sell_on(
    world: &mut LedgerWorld,
    date: String,
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
        FundConfirmation {
            quantity,
            amount_cents,
            fee_cents,
        },
        &account_name,
        None,
        &date,
    );
}

#[when(
    expr = "按确认单出资账户申购基金 {string} 份额 {float} 金额 {int} 手续费 {int} 到投资账户 {string} 出资账户 {string}"
)]
fn fund_buy_with_funding(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: f64,
    amount_cents: i64,
    fee_cents: i64,
    account_name: String,
    funding_name: String,
) {
    fund_trade(
        world,
        TransactionKind::Buy,
        &symbol,
        FundConfirmation {
            quantity,
            amount_cents,
            fee_cents,
        },
        &account_name,
        Some(&funding_name),
        "2026-01-10",
    );
}

fn assert_fund_trade_detail(
    world: &LedgerWorld,
    symbol: &str,
    quantity: f64,
    nav_price_cents: i64,
    fee_cents: i64,
) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有最近的申赎交易");
    let expected_instrument_id = instrument_id_by_symbol(&world_conn!(world), symbol);
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

#[then(expr = "该买入 account_id 应匹配账户 {string}")]
fn check_buy_investment_account(world: &mut LedgerWorld, account_name: String) {
    let txn = world.txn.transactions_list.last().expect("交易列表为空");
    let expected_id = world.account_id(&account_name);
    assert_eq!(
        txn.account_id.as_deref(),
        Some(expected_id.as_str()),
        "买入行的投资账户端不符"
    );
}

#[then(expr = "该买入 funding_account_id 应匹配账户 {string}")]
fn check_buy_funding_account(world: &mut LedgerWorld, account_name: String) {
    let txn = world.txn.transactions_list.last().expect("交易列表为空");
    let expected_id = world.account_id(&account_name);
    assert_eq!(
        txn.funding_account_id.as_deref(),
        Some(expected_id.as_str()),
        "买入行的出资账户端不符"
    );
}
