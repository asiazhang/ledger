use rusqlite::Connection;
use rusqlite::params;
use serde::Serialize;

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::Result;
use ledger_investment::upsert_auto_exchange_rate;
use ledger_sync_protocol::device::device_id;

use super::ecb::FxPairWeeklySeries;
use super::weekly::commit_fx_rate_history_weekly;

pub(super) const ECB_FX_SOURCE: &str = "ecb";

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct FxPersistReport {
    pub pairs: usize,
    pub points: usize,
    pub earliest: Option<String>,
    pub latest: Option<String>,
    pub manual_protected: usize,
}

pub(super) fn persist_ecb_fx_series(
    conn: &Connection,
    series: &[FxPairWeeklySeries],
) -> Result<FxPersistReport> {
    ensure_transaction(conn, || {
        let mut report = FxPersistReport::default();
        for s in series {
            if s.points.is_empty() {
                continue;
            }
            report.pairs += 1;
            commit_fx_rate_history_weekly(conn, &s.base, &s.quote, ECB_FX_SOURCE, &s.points)?;
            for (trade_date, _rate) in &s.points {
                let trade_date = trade_date.format("%Y-%m-%d").to_string();
                if report
                    .earliest
                    .as_ref()
                    .is_none_or(|e| trade_date.as_str() < e.as_str())
                {
                    report.earliest = Some(trade_date.clone());
                }
                if report
                    .latest
                    .as_ref()
                    .is_none_or(|l| trade_date.as_str() > l.as_str())
                {
                    report.latest = Some(trade_date.clone());
                }
            }
            report.points += s.points.len();
            let Some((latest_date, latest_rate)) = s.points.last() else {
                continue;
            };
            let written = upsert_auto_exchange_rate(
                conn,
                &s.base,
                &s.quote,
                *latest_rate,
                &latest_date.format("%Y-%m-%d").to_string(),
                ECB_FX_SOURCE,
            )?;
            if !written {
                report.manual_protected += 1;
            }
        }
        Ok(report)
    })
}

pub(super) fn upsert_fx_rate_history(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    trade_date: &str,
    rate: f64,
    source: &str,
) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7,1,?8) \
         ON CONFLICT(base_code, quote_code, week_start) DO UPDATE SET \
         trade_date=excluded.trade_date, rate=excluded.rate, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![new_uuid(), base_code, quote_code, trade_date, rate, source, now, device_id(conn)?],
    )?;
    Ok(())
}
