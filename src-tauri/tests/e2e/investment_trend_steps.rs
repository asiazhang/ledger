//! 组合走势（PortfolioValueTrend）e2e 步骤与走势类夹具基建（issue #248）。
//!
//! 夹具设计为可复用、不绑定单一场景（后续走势类行为变更的 BDD 落点）：
//!
//! - **价格历史**：经投资域周采样写入单点 [`upsert_price_history`]（#764 旁路
//!   收敛；整周覆盖语义由域函数承担，V010 `week_start` 生成列由 trade_date 派生）。
//! - **汇率历史**：保留直置（#764 已登记例外）——唯一落库点在 sync 持久化模块
//!   内部（`pub(super)`，采集通道需 HTTP），无域层公开写入入口。
//! - **买卖流水**：经真实写路径 `create_transaction_internal`（行为层
//!   plan → apply，ADR-0032 统一写入口），日期参数化以错开周采样键。
//! - **软删账户**：复用 `删除账户` 步骤（accounts_steps，走真实
//!   `delete_account_internal`）。
//! - **组合走势查询**：走 `investment::query_portfolio_value_trend`——与 IPC 命令
//!   `portfolio_value_trend` 同一实现（#401 域目录化后直调域入口）。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::investment::prices::upsert_price_history;
use tauri_app_lib::investment::{
    TrendRange, query_instrument_price_trend, query_portfolio_value_trend,
};
use tauri_app_lib::transaction::TransactionKind;
use tauri_app_lib::transaction::create_transaction_internal;

use crate::common::instrument_id_by_symbol;
use crate::step_inputs::trade_input;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given：行情 / 汇率历史夹具（价格历史经域写入单点；汇率历史无公开入口直置）
// ---------------------------------------------------------------------------

/// 写入一条价格历史周点行：经投资域周采样写入单点 [`upsert_price_history`]（#764
/// 旁路收敛；同标的同周重复写入按「整周覆盖」幂等，与库层 UNIQUE 约束一致）；
/// source 落 'eastmoney' 与原直插同值（同步来源语义）。
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

/// 直插一条汇率历史周点行（1 base = rate quote；同期折算走本表，不用当期汇率近似）。
/// 库内状态直置（#764 已登记例外）：唯一落库点 `sync::persist::upsert_fx_rate_history`
/// 为 `pub(super)` 模块私有接缝（采集通道需 HTTP），域层无公开写入入口，
/// 公开入口表达不了，保留直置。
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

/// 经行为层创建一笔买入/卖出（plan → insert → apply，与 IPC 创建命令同一实现）：
/// L1 买卖工厂构造（行金额置零、单价进 wire；币种/手续费后端以账户币种与重算为准，
/// 提交值不落行——#761 迁移同款裁定）。
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

/// 单标的走势：PriceHistory 直出（基金单位净值即价格，与股票同一承载线，
/// 查询侧不感知标的类型——净值走势即此，issue #303）。
#[when(expr = "查询标的 {string} 的走势")]
fn query_instrument_trend(world: &mut LedgerWorld, symbol: String) {
    let id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    match query_instrument_price_trend(&world_conn!(world), &id, &TrendRange::default()) {
        Ok(trend) => {
            world.asset.last_instrument_trend = Some(trend);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.asset.last_instrument_trend = None;
        }
    }
}

#[then(expr = "标的走势应有 {int} 个周点")]
fn assert_instrument_trend_point_count(world: &mut LedgerWorld, expected: usize) {
    let trend = world
        .asset
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
