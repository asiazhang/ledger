//! 行情同步持久化（issue #89）：`instruments` / `market_prices` 的读写。
//! 与 HTTP、编排无关，仅消费解析后的条目落库。

use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::params;

use crate::db::{device_id, new_uuid, now_iso};
use crate::error::Result;

use super::http::{StockItem, f2_to_cents};

/// 按 (标的, ISO 周) 插入或覆盖一条周采样价格历史（issue #137 / ADR-0019）。
/// 「整周覆盖」幂等由 UNIQUE(instrument_id, week_start)（week_start 为生成列）保证：
/// 同周任一采样日写入都落在同一行上，重复回填零重复行。清仓不删历史（仅随标的删除级联）。
pub(super) fn upsert_price_history(
    conn: &Connection,
    instrument_id: &str,
    trade_date: &str,
    price_cents: i64,
    currency: &str,
) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,?7) \
         ON CONFLICT(instrument_id, week_start) DO UPDATE SET \
         trade_date=excluded.trade_date, price_cents=excluded.price_cents, \
         currency_code=excluded.currency_code, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![new_uuid(), instrument_id, trade_date, price_cents, currency, now, device_id()],
    )?;
    Ok(())
}

/// 按 (币种对, ISO 周) 插入或覆盖一条周采样汇率历史，规则与 [`upsert_price_history`] 对齐
/// （同周整周覆盖、同期采集）。`rate` 口径与 exchange_rates 一致：1 base = ? quote。
pub(super) fn upsert_fx_rate_history(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    trade_date: &str,
    rate: f64,
) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,?7) \
         ON CONFLICT(base_code, quote_code, week_start) DO UPDATE SET \
         trade_date=excluded.trade_date, rate=excluded.rate, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![new_uuid(), base_code, quote_code, trade_date, rate, now, device_id()],
    )?;
    Ok(())
}

/// K 线收盘价（真实价格值）→ 整数分。A 股/港股一致 ×100，与既有 f2 换算结果相同
/// （A 股 f2=价格×100 直接得分；港股 f2=价格×1000 ÷10 亦即 ×100）。
pub(super) fn kline_close_to_cents(close: f64) -> i64 {
    (close * 100.0).round() as i64
}

/// 已存在股票标的的映射值：(id, name, market)，键为 symbol。
pub(super) type ExistingInstrument = (String, Option<String>, String);

/// 按 instrument_id 插入或更新一条行情价格（东财数据源）。
pub(super) fn upsert_market_price(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    currency: &str,
) -> Result<()> {
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM market_prices WHERE instrument_id=?1",
            params![instrument_id],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or_else(new_uuid);
    let now = now_iso();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?7,?8,?9) \
         ON CONFLICT(instrument_id) DO UPDATE SET \
         price_cents=excluded.price_cents, currency_code=excluded.currency_code, \
         priced_at=excluded.priced_at, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![id, instrument_id, price_cents, currency, now, now, now, 1, device_id()],
    )?;
    Ok(())
}

/// 构建现有股票标的映射：symbol → (id, name, market)。
pub(super) fn build_existing_instruments(
    conn: &Connection,
) -> Result<HashMap<String, ExistingInstrument>> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol, name, market FROM instruments WHERE instrument_type='stock'",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(1)?,
            (
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ),
        ))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (symbol, entry) = row?;
        map.insert(symbol, entry);
    }
    Ok(map)
}

/// 应用单个行情条目：标的已存在则更新名称/市场（必要时）并 upsert 价格，
/// 不存在则插入新标的并 upsert 价格。返回 `(新增数, 更新数)`。
pub(super) fn apply_stock_item(
    conn: &Connection,
    item: &StockItem,
    market_code: &str,
    currency: &str,
    existing_map: &mut HashMap<String, ExistingInstrument>,
) -> Result<(usize, usize)> {
    if let Some((existing_id, existing_name, existing_market)) = existing_map.get(&item.code) {
        let name_changed = item.name != existing_name.as_deref().unwrap_or("");
        let market_changed = market_code != existing_market.as_str();
        let mut updated = 0usize;
        if name_changed || market_changed {
            let now = now_iso();
            conn.execute(
                "UPDATE instruments SET name=?1, market=?2, updated_at=?3, version=version+1 WHERE id=?4",
                params![item.name, market_code, now, existing_id],
            )?;
            updated = 1;
        }
        if let Some(raw) = item.price {
            let price = f2_to_cents(raw, market_code);
            upsert_market_price(conn, existing_id, price, currency)?;
        }
        Ok((0, updated))
    } else {
        let id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'stock',?3,?4,?5,?6,?7,?8,?9)",
            params![
                id,
                item.code,
                item.name,
                currency,
                market_code,
                now,
                now,
                1,
                device_id()
            ],
        )?;
        if let Some(raw) = item.price {
            let price = f2_to_cents(raw, market_code);
            upsert_market_price(conn, &id, price, currency)?;
        }
        existing_map.insert(
            item.code.clone(),
            (id.clone(), Some(item.name.clone()), market_code.to_string()),
        );
        Ok((1, 0))
    }
}
