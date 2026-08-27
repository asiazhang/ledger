//! 持仓价格增量同步编排（issue #103）：从当前持仓收集股票类标的，按批次向东方财富
//! 批量报价接口查询最新价，**仅 upsert `market_prices`**——不增删、不改标的字典
//! （名称/市场/数量）。职责切分：全量同步修字典 / 增量同步刷价格（issue #101）。
//!
//! 编排与网络解耦：核心流程 [`do_incremental_sync_with`] 接受注入的批量查询函数，
//! 测试以 mock 数据驱动（不依赖真实网络）；生产经 [`do_incremental_sync`] 接 HTTP 层
//! （复用主机池/重试/限流 pacer 与价格换算）。

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::error::Result;
use crate::models::SyncHoldingPricesResult;

use super::http::{
    Pacer, StockItem, ULIST_BATCH_SIZE, build_client, f2_to_cents, fetch_ulist, secid_prefix,
};
use super::persist::upsert_market_price;

/// 持仓股票的报价代码：东财 secid 与响应 f12 均为裸代码（如 600519 / 00700）。
/// 字典 symbol 可能带市场后缀（schema 注释示例格式如 "600519.SH"），取点号前段归一化。
fn quote_code(symbol: &str) -> &str {
    symbol.split('.').next().unwrap_or(symbol)
}

/// 待同步的持仓股票元信息（一个标的一条，跨账户去重）。
struct HeldStock {
    instrument_id: String,
    symbol: String,
    market: String,
    currency: String,
}

/// 从 v_holdings 收集当前持仓的股票类标的（口径与「持仓标的」一致：有当前持仓批次、
/// 排除软删除账户），按标的去重。非股票持仓不在此列，由 [`count_non_stock_holdings`] 另行计数。
fn collect_held_stocks(conn: &Connection) -> Result<Vec<HeldStock>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT i.id, i.symbol, i.market, i.currency_code \
         FROM v_holdings h \
         JOIN instruments i ON i.id = h.instrument_id \
         WHERE i.instrument_type = 'stock' \
         ORDER BY i.symbol",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(HeldStock {
            instrument_id: r.get(0)?,
            symbol: r.get(1)?,
            market: r.get(2)?,
            currency: r.get(3)?,
        })
    })?;
    let mut stocks = Vec::new();
    for row in rows {
        stocks.push(row?);
    }
    Ok(stocks)
}

/// 非股票类持仓数（基金/债券/ETF/其他），计入跳过统计（数据源不含此类行情）。
fn count_non_stock_holdings(conn: &Connection) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT i.id) \
         FROM v_holdings h \
         JOIN instruments i ON i.id = h.instrument_id \
         WHERE i.instrument_type != 'stock'",
        [],
        |r| r.get(0),
    )?;
    Ok(count as usize)
}

/// 增量同步核心流程：收集持仓股票 → 构造 secid → 按批查询 → 仅 upsert 价格 → 结果统计。
/// `fetch` 由调用方注入（生产接 HTTP 层，测试注入 mock），本函数不触碰网络。
/// 返回 `(同步数, 跳过数)`，跳过数 = 非股票持仓 + 无法构造 secid（市场未知）+ 无效价/查询无果。
pub(super) fn do_incremental_sync_with<F>(
    conn: &Connection,
    fetch: &mut F,
) -> Result<SyncHoldingPricesResult>
where
    F: FnMut(&str) -> Result<Vec<StockItem>>,
{
    let stocks = collect_held_stocks(conn)?;
    let non_stock = count_non_stock_holdings(conn)?;

    // 完全无持仓：明确提示，不报错。
    if stocks.is_empty() && non_stock == 0 {
        return Ok(SyncHoldingPricesResult {
            synced: 0,
            skipped: 0,
            message: "无持仓标的可同步".into(),
        });
    }

    // 构造可查询 secid 与报价代码 → 持仓股票 映射。键为报价代码（已归一化，与响应 f12 对齐）；
    // 股票内 symbol 唯一（instruments 的 UNIQUE(symbol, instrument_type)），同代码不冲突。
    // 市场未知（unknown）无法构造 secid，计入跳过。
    let mut meta: HashMap<String, &HeldStock> = HashMap::new();
    let mut queryable: Vec<String> = Vec::new();
    let mut skipped_unqueryable = 0usize;
    for stock in &stocks {
        if let Some(prefix) = secid_prefix(&stock.market) {
            let code = quote_code(&stock.symbol);
            meta.insert(code.to_string(), stock);
            queryable.push(format!("{prefix}.{code}"));
        } else {
            skipped_unqueryable += 1;
        }
    }

    // 按批查询并仅 upsert 价格（幂等：每标的一条 market_prices 覆盖更新，不产生重复数据）。
    let mut synced_codes: HashSet<String> = HashSet::new();
    for chunk in queryable.chunks(ULIST_BATCH_SIZE) {
        let items = fetch(&chunk.join(","))?;
        for item in &items {
            if let Some(stock) = meta.get(&item.code) {
                // f2≤0（停牌/无效价）经 deserialize_f2 已过滤为 None，此处跳过、保留旧价。
                if let Some(raw) = item.price {
                    let cents = f2_to_cents(raw, &stock.market);
                    upsert_market_price(conn, &stock.instrument_id, cents, &stock.currency)?;
                    synced_codes.insert(item.code.clone());
                }
            }
        }
    }

    let synced = synced_codes.len();
    // 已查询但未取到有效价的（停牌/无效价/查询无果）计入跳过。
    let invalid = queryable.len() - synced;
    let skipped = non_stock + skipped_unqueryable + invalid;

    Ok(SyncHoldingPricesResult {
        synced,
        skipped,
        message: format!("已同步 {synced} 只，跳过 {skipped} 只"),
    })
}

/// 生产入口：接 HTTP 层的批量报价查询（复用主机池、重试、限流 pacer 与价格换算）。
pub(super) fn do_incremental_sync(conn: &Connection) -> Result<SyncHoldingPricesResult> {
    let client = build_client()?;
    let mut pacer = Pacer::default();
    let mut fetch = |secids: &str| fetch_ulist(&client, &mut pacer, secids);
    do_incremental_sync_with(conn, &mut fetch)
}
