use cucumber::{given, then, when};
use rusqlite::params;

use ledger_infra::db::{new_uuid, now_iso};
use ledger_investment::prices::upsert_price_history;
use ledger_investment::{TrendRange, query_portfolio_value_trend};
use ledger_transaction::TransactionKind;
use ledger_transaction::create_transaction_internal;

use crate::common::instrument_id_by_symbol;
use crate::step_inputs::trade_input;
use crate::world::LedgerWorld;

#[given(expr = "存在标的 {string} 的价格历史 交易日 {string} 价格 {int} 万分之一元 币种 {string}")]
fn add_price_history(
    world: &mut LedgerWorld,
    symbol: String,
    trade_date: String,
    price_cents: i64,
    currency: String,
) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    upsert_price_history(
        &world_conn!(world),
        &instrument_id,
        &trade_date,
        price_cents,
        &currency,
        "eastmoney",
    )
    .expect("价格历史夹具：写入失败");
}

#[given(expr = "存在汇率历史 {string} 兑 {string} 交易日 {string} 汇率 {float}")]
fn add_fx_rate_history(
    world: &mut LedgerWorld,
    base: String,
    quote: String,
    trade_date: String,
    rate: f64,
) {
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,?7)",
            params![new_uuid(), base, quote, trade_date, rate, now, "e2e-fixture"],
        )
        .unwrap();
}

#[when(expr = "买入标的 {string} 数量 {float} 单价 {int} 到账户 {string} 日期 {string}")]
fn buy_instrument_on(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: f64,
    price_cents: i64,
    account_name: String,
    date: String,
) {
    create_trade(
        world,
        TransactionKind::Buy,
        &symbol,
        quantity,
        price_cents,
        &account_name,
        &date,
    );
}

#[when(expr = "卖出标的 {string} 数量 {float} 单价 {int} 从账户 {string} 日期 {string}")]
fn sell_instrument_on(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: f64,
    price_cents: i64,
    account_name: String,
    date: String,
) {
    create_trade(
        world,
        TransactionKind::Sell,
        &symbol,
        quantity,
        price_cents,
        &account_name,
        &date,
    );
}

fn create_trade(
    world: &mut LedgerWorld,
    kind: TransactionKind,
    symbol: &str,
    quantity: f64,
    price_cents: i64,
    account_name: &str,
    date: &str,
) {
    let account_id = world.account_id(account_name);
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), symbol);
    let input = trade_input(
        kind,
        &instrument_id,
        quantity,
        price_cents,
        &account_id,
        date,
    );
    let result = world_write!(world, |conn| create_transaction_internal(conn, input));
    assert!(result.is_ok(), "创建 {kind:?} 交易失败: {:?}", result.err());
}

#[when(expr = "查询组合走势")]
fn query_portfolio_trend(world: &mut LedgerWorld) {
    match query_portfolio_value_trend(&world_conn!(world), &TrendRange::default()) {
        Ok(trend) => {
            world.asset.last_portfolio_trend = Some(trend);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.asset.last_portfolio_trend = None;
        }
    }
}

#[then(expr = "组合走势应有 {int} 个周点")]
fn assert_portfolio_trend_point_count(world: &mut LedgerWorld, expected: usize) {
    let trend = world
        .asset
        .last_portfolio_trend
        .as_ref()
        .expect("未查询组合走势");
    assert_eq!(
        trend.points.len(),
        expected,
        "组合走势周点数不符：{trend:?}"
    );
}

#[then(expr = "组合走势 {string} 周市值应为 {int}")]
fn assert_portfolio_trend_week_value(world: &mut LedgerWorld, week_start: String, expected: i64) {
    let trend = world
        .asset
        .last_portfolio_trend
        .as_ref()
        .expect("未查询组合走势");
    let point = trend
        .points
        .iter()
        .find(|p| p.date == week_start)
        .unwrap_or_else(|| panic!("组合走势无 {week_start} 周点：{trend:?}"));
    assert_eq!(
        point.market_value_cents, expected,
        "组合走势 {week_start} 周市值不符：{trend:?}"
    );
}
