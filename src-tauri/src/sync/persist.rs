//! 行情同步持久化（issue #137）：汇率历史周采样落库。
//! 价格写入单点（现价缓存 / 价格历史周采样 upsert 与刻度换算）已随投资域归位
//! 迁入 [`crate::investment::prices`]（#401 / ADR-0056），增量同步经域入口消费；
//! 标的字典应用（全量同步侧）已随 ADR-0081 决策 3 退役删除（issue #698）。

use rusqlite::Connection;
use rusqlite::params;

use crate::db::{new_uuid, now_iso};
use crate::error::Result;
use ledger_sync_protocol::device::device_id;

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
        params![new_uuid(), base_code, quote_code, trade_date, rate, now, device_id(conn)?],
    )?;
    Ok(())
}
