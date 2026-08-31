//! 手动报价 e2e 步骤（issue #291 / ADR-0036）：录价写入走
//! `commands::investment::record_manual_price_internal`（与 IPC 命令同一实现），
//! 断言读现价缓存 / 价格历史 / `v_holdings` 视图。组合走势、净资产总览与买卖
//! 流水复用既有步骤（investment_trend_steps / dashboard_steps / instruments_steps）。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::commands::investment::record_manual_price_internal;
use tauri_app_lib::models::ManualPriceInput;

use crate::world::LedgerWorld;

/// 按标的代码查 instrument id（手动创建步骤先行落库，必存在）。
fn instrument_id(conn: &rusqlite::Connection, symbol: &str) -> String {
    conn.query_row(
        "SELECT id FROM instruments WHERE symbol=?1",
        params![symbol],
        |r| r.get(0),
    )
    .unwrap_or_else(|_| panic!("标的不存在，先铺垫手动创建标的步骤: {symbol}"))
}

// ---------------------------------------------------------------------------
// When：录价（真实写路径，与 IPC 命令同一实现）
// ---------------------------------------------------------------------------

#[when(expr = "给标的 {string} 录价 日期 {string} 价格 {int} 万分之一元")]
fn record_manual_quote(world: &mut LedgerWorld, symbol: String, date: String, price_cents: i64) {
    let input = ManualPriceInput {
        instrument_id: instrument_id(&world_conn!(world), &symbol),
        date,
        price_cents,
    };
    let result = record_manual_price_internal(&world_conn!(world), &input);
    match result {
        Ok(_) => world.last_error = None,
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Then：现价缓存 / 价格历史 / 持仓视图
// ---------------------------------------------------------------------------

/// 现价缓存断言（不限标的类型）：手动报价落点一——来源 manual、
/// priced_at = 报价日、无净值日期语义。
#[then(expr = "标的 {string} 现价为 {int} 万分之一元 priced_at {string} 来源 {string}")]
fn assert_market_price(
    world: &mut LedgerWorld,
    symbol: String,
    price_cents: i64,
    priced_at: String,
    source: String,
) {
    let row: Option<(i64, String, Option<String>)> = world_conn!(world)
        .query_row(
            "SELECT p.price_cents, p.priced_at, p.nav_date \
             FROM market_prices p JOIN instruments i ON i.id = p.instrument_id \
             WHERE i.symbol=?1",
            params![symbol],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let (actual_price, actual_priced_at, nav_date) =
        row.unwrap_or_else(|| panic!("标的 {symbol} 应有现价缓存"));
    assert_eq!(actual_price, price_cents, "现价（万分之一元）不符");
    assert_eq!(actual_priced_at, priced_at, "现价 priced_at 不符");
    assert_eq!(nav_date, None, "手动落价无净值日期语义，nav_date 应为 NULL");
    // 来源标记单列断言（与价格值同行，分开查询以简化元组）。
    let actual_source: String = world_conn!(world)
        .query_row(
            "SELECT p.source FROM market_prices p JOIN instruments i ON i.id = p.instrument_id \
             WHERE i.symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(actual_source, source, "现价来源标记不符");
}

#[then(expr = "标的 {string} 价格历史应有 {int} 条")]
fn assert_price_history_count(world: &mut LedgerWorld, symbol: String, count: i64) {
    let instrument_id = instrument_id(&world_conn!(world), &symbol);
    let actual: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM price_history WHERE instrument_id=?1",
            params![instrument_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(actual, count, "标的 {symbol} 价格历史条数不符");
}

/// 周点断言按周键（week_start 生成列）定位：该报价日所在 ISO 周至多一条，
/// 整周覆盖后 trade_date 为该周最后写入的报价日。
#[then(expr = "标的 {string} 价格历史 {string} 周点价格为 {int} 万分之一元 来源 {string}")]
fn assert_price_history_week_point(
    world: &mut LedgerWorld,
    symbol: String,
    any_day_in_week: String,
    price_cents: i64,
    source: String,
) {
    let instrument_id = instrument_id(&world_conn!(world), &symbol);
    let row: Option<(String, i64, String)> = world_conn!(world)
        .query_row(
            "SELECT trade_date, price_cents, source FROM price_history \
             WHERE instrument_id=?1 AND week_start = date(?2,'-6 days','weekday 1')",
            params![instrument_id, any_day_in_week],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let (trade_date, actual_price, actual_source) =
        row.unwrap_or_else(|| panic!("标的 {symbol} 在 {any_day_in_week} 所在周应有一条周点"));
    assert_eq!(
        actual_price, price_cents,
        "周点价格不符（trade_date {trade_date}）"
    );
    assert_eq!(actual_source, source, "周点来源标记不符");
}

#[then(expr = "标的 {string} 持仓视图市值应为 {int}")]
fn assert_holding_market_value(world: &mut LedgerWorld, symbol: String, expected: i64) {
    let actual: Option<i64> = world_conn!(world)
        .query_row(
            "SELECT h.market_value_cents FROM v_holdings h \
             JOIN instruments i ON i.id = h.instrument_id WHERE i.symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .ok();
    let actual = actual.unwrap_or_else(|| panic!("标的 {symbol} 应有持仓视图行（v_holdings）"));
    assert_eq!(actual, expected, "标的 {symbol} 持仓市值不符");
}

#[then(expr = "标的 {string} 持仓视图未实现盈亏应为 {int}")]
fn assert_holding_unrealized_pnl(world: &mut LedgerWorld, symbol: String, expected: i64) {
    let actual: Option<i64> = world_conn!(world)
        .query_row(
            "SELECT h.unrealized_pnl_cents FROM v_holdings h \
             JOIN instruments i ON i.id = h.instrument_id WHERE i.symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .ok();
    let actual = actual.unwrap_or_else(|| panic!("标的 {symbol} 应有持仓视图行（v_holdings）"));
    assert_eq!(actual, expected, "标的 {symbol} 未实现盈亏不符");
}
