//! 场外基金申赎记账 BDD 步骤（issue #302 / ADR-0038 金额权威）：确认单整分金额 +
//! 确认份额为权威输入（wire 不携带单价，由后端行为层反算净值），持仓与已实现
//! 盈亏读回断言（闭合不变式：全平仓 Σ 已实现盈亏 = Σ 卖出金额 − Σ 买入金额）。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::investment::{InstrumentInput, InstrumentType, create_instrument};
use tauri_app_lib::transaction::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::create_transaction_internal;

use crate::common::{instrument_id_by_symbol, query_all_transactions};
use crate::step_inputs::{buy_input, sell_input};
use crate::world::LedgerWorld;

/// 经投资域核心创建入口建基金标的字典行（#763 旁路归零；基金不设手动白名单，
/// 核心入口与 AI HTTP 端点同款五类全开；场景无来源断言，来源标记不涉被测语义）。
#[given(expr = "存在基金标的 {string} 名称 {string}")]
fn create_fund_instrument(world: &mut LedgerWorld, symbol: String, name: String) {
    let input = InstrumentInput {
        symbol: symbol.clone(),
        kind: InstrumentType::Fund,
        name: Some(name),
        currency_code: "CNY".into(),
        market: None,
    };
    world
        .db
        .write(|conn| create_instrument(conn, input))
        .expect("新建基金标的失败");
}

/// 确认单三联（issue #302 金额权威）：份额、整分金额、手续费——基金申赎步骤的
/// 热点数值入参打包，避免步骤助手长参数列表。
struct FundConfirmation {
    quantity: f64,
    amount_cents: i64,
    fee_cents: i64,
}

/// 按确认单录入基金申赎（金额权威：amount_cents = 确认单整分金额，wire 不带单价）。
/// `funding_name` 非空时携带出资账户（issue #937 / ADR-0096：直扣买卖的资金流归因），
/// 经同一行为层公开创建入口写入。交易日由调用方显式给出（issue #982）：转换旅程的
/// 申赎须与转换日构成先后（FIFO 消耗序），既有步骤的固定日期仅是缺省捷径。
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
    // 金额权威（issue #302 / ADR-0038）：确认单整分金额 + 手续费进 wire，
    // 单价不落 wire（L1 买卖工厂金额置零 + 单价权威为默认，基金经结构体
    // 更新改走金额权威——step_inputs::buy_input 文档载明的基金形态）。
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

/// 带交易日形态（issue #982）：转换旅程的申购日须先于转换日（FIFO 消耗序）。
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

/// 带交易日形态（issue #982）：全平仓旅程的赎回日在转换之后。
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

/// 直扣申购（issue #937 / ADR-0096）：出资账户命中的买入——结算现金从出资账户
/// 流出、投资账户现金腿为 0。经同一行为层公开创建入口写入（公开写入口，非裸 SQL）。
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

/// 断言最近一笔申赎的明细（扩展表投影：份额/反算净值/手续费）。
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

/// 列表两端展示断言（issue #937 / ADR-0096）：出资账户命中的 buy 行读回投影
/// 同时携带投资账户（account_id）与出资账户（funding_account_id）——列表双链接
/// 与下钻的行级数据前提；「转出 → 转入」断言（transactions_write_steps）同形。
#[then(expr = "该买入 account_id 应匹配账户 {string}")]
fn check_buy_investment_account(world: &mut LedgerWorld, account_name: String) {
    let txn = world.txn.transactions_list.last().expect("交易列表为空");
    let expected_id = world.account_id(&account_name);
    assert_eq!(txn.account_id, expected_id, "买入行的投资账户端不符");
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
