//! 行情同步持久化（issue #89）：标的字典的应用与汇率历史周采样落库。
//! 价格写入单点（现价缓存 / 价格历史周采样 upsert 与刻度换算）已随投资域归位
//! 迁入 [`crate::investment::prices`]（#401 / ADR-0056），本模块经域入口消费。

use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::params;

use crate::db::{device_id, new_uuid, now_iso};
use crate::error::Result;
use crate::investment::prices::{EASTMONEY_PRICE_SOURCE, upsert_market_price};

use super::http::{StockItem, f2_to_price};

/// 按 (币种对, ISO 周) 插入或覆盖一条周采样汇率历史，规则与投资域价格历史
/// 周采样 upsert（[`crate::investment::prices::upsert_price_history`]）对齐
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

/// 已存在股票标的的映射值：(id, name, market)，键为 symbol。
pub(super) type ExistingInstrument = (String, Option<String>, String);

/// 构建现有股票标的映射：symbol → (id, name, market)。构造后由全量同步逐条
/// 消费（[`apply_stock_item`]）；行情价格落库经投资域写入单点
/// [`crate::investment::prices::upsert_market_price`]。
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
            let price = f2_to_price(raw, market_code);
            upsert_market_price(
                conn,
                existing_id,
                price,
                currency,
                &now_iso(),
                None,
                Some(EASTMONEY_PRICE_SOURCE),
            )?;
        }
        Ok((0, updated))
    } else {
        let id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
             VALUES (?1,?2,'stock',?3,?4,?5,?6,?7,?8,?9,'eastmoney')",
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
            let price = f2_to_price(raw, market_code);
            upsert_market_price(
                conn,
                &id,
                price,
                currency,
                &now_iso(),
                None,
                Some(EASTMONEY_PRICE_SOURCE),
            )?;
        }
        existing_map.insert(
            item.code.clone(),
            (id.clone(), Some(item.name.clone()), market_code.to_string()),
        );
        Ok((1, 0))
    }
}
