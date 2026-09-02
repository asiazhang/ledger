//! 组合走势（PortfolioValueTrend）e2e 步骤与走势类夹具基建（issue #248）。
//!
//! 夹具设计为可复用、不绑定单一场景（后续走势类行为变更的 BDD 落点）：
//!
//! - **价格历史/汇率历史**：采集通道是行情同步（需 HTTP），走势查询为只读，
//!   故按单测先例直插周点行（V010 `week_start` 生成列由 trade_date 派生）。
//! - **买卖流水**：经真实写路径 `create_transaction_internal`（行为层
//!   plan → apply，ADR-0032 统一写入口），日期参数化以错开周采样键。
//! - **软删账户**：复用 `删除账户` 步骤（accounts_steps，走真实
//!   `delete_account_internal`）。
//! - **组合走势查询**：走 `investment::query_portfolio_value_trend`——与 IPC 命令
//!   `portfolio_value_trend` 同一实现（#401 域目录化后直调域入口）。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::investment::{query_instrument_price_trend, query_portfolio_value_trend};
use tauri_app_lib::models::{TransactionInput, TrendRange};
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::create_transaction_internal;

use crate::world::LedgerWorld;

/// 按标的代码查 instrument id（Given 步骤先行落库，必存在）。
fn instrument_id(conn: &rusqlite::Connection, symbol: &str) -> String {
    conn.query_row(
        "SELECT id FROM instruments WHERE symbol=?1",
        params![symbol],
        |r| r.get(0),
    )
    .unwrap_or_else(|_| panic!("标的不存在，先铺垫存在标的步骤: {symbol}"))
}

// ---------------------------------------------------------------------------
// Given：行情 / 汇率历史夹具（直插周点行，采集通道需 HTTP 故绕过）
// ---------------------------------------------------------------------------

/// 直插一条价格历史周点行（`week_start` 为 STORED 生成列，随 trade_date 派生；
/// 同标的同周重复插入撞 UNIQUE，与「整周覆盖」的库层约束一致）。
#[given(expr = "存在标的 {string} 的价格历史 交易日 {string} 价格 {int} 万分之一元 币种 {string}")]
fn add_price_history(
    world: &mut LedgerWorld,
    symbol: String,
    trade_date: String,
    price_cents: i64,
    currency: String,
) {
    let instrument_id = instrument_id(&world_conn!(world), &symbol);
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,?7)",
            params![new_uuid(), instrument_id, trade_date, price_cents, currency, now, device_id()],
        )
        .unwrap();
}

/// 直插一条汇率历史周点行（1 base = rate quote；同期折算走本表，不用当期汇率近似）。
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
            params![new_uuid(), base, quote, trade_date, rate, now, device_id()],
        )
        .unwrap();
}

// ---------------------------------------------------------------------------
// When：日期参数化的买卖流水（真实写路径，错开周采样键用）
// ---------------------------------------------------------------------------

/// 经行为层创建一笔买入（plan → insert → apply，与 IPC 创建命令同一实现）。
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

/// 经行为层创建一笔卖出（卖出匹配走投资域 apply/revert 三件套）。
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

/// 买卖流水共用写入：以账户币种成交（fixture 入参与真实写路径一致）。
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
    let (instrument_id, currency_code) = {
        let conn = world_conn!(world);
        let instrument_id = instrument_id(&conn, symbol);
        let currency_code: String = conn
            .query_row(
                "SELECT currency_code FROM accounts WHERE id=?1",
                params![account_id],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| panic!("账户不存在: {account_name}"));
        (instrument_id, currency_code)
    };
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind,
        // 占位金额（prepare 按「数量 × 单价（万分之一元）÷ 100 ± 手续费」重算覆盖）
        amount_cents: (quantity * price_cents as f64 / 100.0).round() as i64,
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
        price_cents: Some(price_cents),
        fee_cents: Some(0),
        idempotency_key: None,
    };
    // 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）创建，提交点置脏/到期检查。
    let result = world
        .db
        .write(|conn| create_transaction_internal(conn, input));
    assert!(result.is_ok(), "创建 {kind:?} 交易失败: {:?}", result.err());
}

// ---------------------------------------------------------------------------
// When / Then：组合走势查询与断言
// ---------------------------------------------------------------------------

#[when(expr = "查询组合走势")]
fn query_portfolio_trend(world: &mut LedgerWorld) {
    match query_portfolio_value_trend(&world_conn!(world), &TrendRange::default()) {
        Ok(trend) => {
            world.last_portfolio_trend = Some(trend);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.last_portfolio_trend = None;
        }
    }
}

#[then(expr = "组合走势应有 {int} 个周点")]
fn assert_portfolio_trend_point_count(world: &mut LedgerWorld, expected: usize) {
    let trend = world.last_portfolio_trend.as_ref().expect("未查询组合走势");
    assert_eq!(
        trend.points.len(),
        expected,
        "组合走势周点数不符：{trend:?}"
    );
}

/// 单标的走势：PriceHistory 直出（基金单位净值即价格，与股票同一承载线，
/// 查询侧不感知标的类型——净值走势即此，issue #303）。
#[when(expr = "查询标的 {string} 的走势")]
fn query_instrument_trend(world: &mut LedgerWorld, symbol: String) {
    let id = instrument_id(&world_conn!(world), &symbol);
    match query_instrument_price_trend(&world_conn!(world), &id, &TrendRange::default()) {
        Ok(trend) => {
            world.last_instrument_trend = Some(trend);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.last_instrument_trend = None;
        }
    }
}

#[then(expr = "标的走势应有 {int} 个周点")]
fn assert_instrument_trend_point_count(world: &mut LedgerWorld, expected: usize) {
    let trend = world
        .last_instrument_trend
        .as_ref()
        .expect("未查询标的走势");
    assert_eq!(
        trend.points.len(),
        expected,
        "标的走势周点数不符：{trend:?}"
    );
}

#[then(expr = "组合走势 {string} 周市值应为 {int}")]
fn assert_portfolio_trend_week_value(world: &mut LedgerWorld, week_start: String, expected: i64) {
    let trend = world.last_portfolio_trend.as_ref().expect("未查询组合走势");
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
