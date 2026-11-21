use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::NaiveDate;
use rusqlite::Connection;

use super::bulk::{BulkFetchSurfaces, BulkNavPoint, FetchFundBatch, FundBatch};
use super::fund::is_fund_not_found;
use super::model::{SyncInstrumentInfoResult, WriteWitness};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};
use ledger_investment::backfill;
use ledger_investment::crud::refresh_instrument_name;
use ledger_investment::prices::{
    MarketPriceWrite, TENCENT_PRICE_SOURCE, price_cents_to_value, upsert_market_price,
};
use ledger_investment::{
    InstrumentType, Market, PriceChannel, QuoteMarket, derive_price_channel, derive_quote_market,
};

use super::channels::{FetchFuture, QuoteItem, QuoteQuery};
use super::fund_nav::{FundIdentity, NavPoint};
use super::fund_price_refresh::{FundSyncStats, refresh_one_fund_price};
use super::progress::SyncProgress;
use super::session::ScopedSession;
use super::weekly::commit_price_history_weekly;

pub(super) fn quote_code(symbol: &str) -> &str {
    symbol.split('.').next().unwrap_or(symbol)
}

pub(super) struct SyncInstrument {
    pub(super) instrument_id: String,
    pub(super) symbol: String,
    pub(super) market: Market,
    pub(super) currency: String,
    pub(super) kind: InstrumentType,
    pub(super) constant_unit_price: Option<i64>,
    pub(super) channel: PriceChannel,
}

fn collect_instruments(conn: &Connection) -> Result<Vec<SyncInstrument>> {
    let sql = "SELECT i.id, i.symbol, i.market, i.currency_code, i.instrument_type, i.constant_unit_price \
               FROM instruments i \
               ORDER BY i.symbol";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        let symbol: String = r.get(1)?;
        let market: Market = r.get(2)?;
        let kind: InstrumentType = r.get(4)?;
        let constant_unit_price: Option<i64> = r.get(5)?;
        Ok(SyncInstrument {
            instrument_id: r.get(0)?,
            channel: derive_price_channel(kind, market, &symbol, constant_unit_price),
            symbol,
            market,
            currency: r.get(3)?,
            kind,
            constant_unit_price,
        })
    })?;
    let mut instruments = Vec::new();
    for row in rows {
        instruments.push(row?);
    }
    Ok(instruments)
}

struct BulkData {
    batch: Option<FundBatch>,
    degraded: bool,
}

impl BulkData {
    fn none() -> Self {
        Self {
            batch: None,
            degraded: false,
        }
    }

    fn unavailable() -> Self {
        Self {
            batch: None,
            degraded: true,
        }
    }

    fn covers(&self, code: &str) -> bool {
        self.batch.as_ref().is_some_and(|batch| batch.covers(code))
    }

    fn nav_of(&self, code: &str) -> Option<&BulkNavPoint> {
        self.batch.as_ref().and_then(|batch| batch.nav_of(code))
    }

    fn name_of(&self, code: &str) -> Option<&str> {
        self.batch.as_ref().and_then(|batch| batch.name_of(code))
    }

    fn has_batch(&self) -> bool {
        self.batch.is_some()
    }
}

async fn fetch_bulk_surfaces(bulk: &mut BulkFetchSurfaces, codes: &[String]) -> BulkData {
    let now = Instant::now();
    let allowed = match bulk.circuit.lock() {
        Ok(mut circuit) => circuit.should_attempt(now),
        Err(error) => {
            tracing::warn!(%error, "批量取数面跨同步记忆互斥体损坏，本次同步按降级处理");
            false
        }
    };
    if !allowed {
        tracing::info!("批量取数面处于停用期（连续失败达阈值），本次同步全部走逐标的通道");
        return BulkData::unavailable();
    }

    let batch = take_bulk_surface(&mut bulk.funds, codes).await;
    let failed = batch.is_none();

    match bulk.circuit.lock() {
        Ok(mut circuit) => {
            if failed {
                circuit.record_failure(now);
            } else {
                circuit.record_success();
            }
        }
        Err(error) => tracing::warn!(%error, "批量取数面跨同步记忆互斥体损坏，本次结果未记入"),
    }

    match batch {
        Some(batch) => BulkData {
            batch: Some(batch),
            degraded: false,
        },
        None => BulkData::unavailable(),
    }
}

async fn take_bulk_surface(fetch: &mut FetchFundBatch, codes: &[String]) -> Option<FundBatch> {
    match fetch(codes).await {
        Ok(batch) => {
            tracing::debug!(
                covered = batch.names.len(),
                nav_points = batch.nav.len(),
                "场外基金批量面命中"
            );
            Some(batch)
        }
        Err(error) => {
            tracing::warn!(%error, "场外基金批量面失败，本次同步回退逐标的通道");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn do_incremental_sync_with<Q, F, H, M, C, P>(
    session: &Q,
    fetch: &mut F,
    fetch_nav_history: &mut H,
    fetch_fund_name: &mut M,
    confirm_money_fund: &mut C,
    bulk: &mut BulkFetchSurfaces,
    progress: &mut P,
    witness: &mut WriteWitness,
) -> Result<SyncInstrumentInfoResult>
where
    Q: ScopedSession,
    F: FnMut(&[QuoteQuery]) -> FetchFuture<Vec<QuoteItem>> + Send,
    H: FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send,
    M: FnMut(&str) -> FetchFuture<String> + Send,
    C: FnMut(&str) -> FetchFuture<bool> + Send,
    P: FnMut(SyncProgress) + Send,
{
    let held = session.with_connection(collect_instruments).await?;
    let quote_channel: Vec<(&SyncInstrument, QuoteMarket)> = held
        .iter()
        .filter_map(|i| {
            let quote_market =
                derive_quote_market(i.kind, i.market, &i.symbol, i.constant_unit_price)?;
            Some((i, quote_market))
        })
        .collect();
    let funds: Vec<&SyncInstrument> = held
        .iter()
        .filter(|i| i.channel == PriceChannel::FundNav)
        .collect();
    let uncollected = held.len() - quote_channel.len() - funds.len();
    if held.is_empty() {
        return Ok(SyncInstrumentInfoResult {
            synced: 0,
            skipped: 0,
            message: "暂无标的可同步".into(),
            written: 0,
            renamed: 0,
            bulk_degraded: false,
            bulk_gaps: 0,
        });
    }

    let mut meta: HashMap<String, &SyncInstrument> = HashMap::new();
    let mut queryable: Vec<QuoteQuery> = Vec::new();
    for (inst, quote_market) in &quote_channel {
        let code = quote_code(&inst.symbol);
        meta.insert(code.to_string(), inst);
        queryable.push(QuoteQuery {
            market: *quote_market,
            code: code.to_string(),
        });
    }

    let total = queryable.len() + funds.len();
    if total > 0 {
        progress(SyncProgress::instrument(0, total));
    }
    let mut done = 0usize;

    let mut synced_codes: HashSet<String> = HashSet::new();
    let mut renamed = 0usize;
    if !queryable.is_empty() {
        let items = fetch(&queryable).await?;
        for item in &items {
            if let Some(inst) = meta.get(&item.code) {
                let marks = Arc::new(Mutex::new(Vec::new()));
                let sink = Arc::clone(&marks);
                let renamed_now = session
                    .with_connection({
                        let instrument_id = inst.instrument_id.clone();
                        let currency = inst.currency.clone();
                        let name = item.name.clone();
                        let price_cents = item.price_cents;
                        let price_date = item.price_date.clone();
                        move |conn| {
                            if refresh_instrument_name(conn, &instrument_id, &name)? {
                                mark(&sink, StockMark::Renamed);
                            }
                            if let Some(price_cents) = price_cents {
                                upsert_market_price(
                                    conn,
                                    &MarketPriceWrite {
                                        instrument_id: &instrument_id,
                                        price_cents,
                                        currency_code: &currency,
                                        priced_at: &ledger_infra::db::now_iso(),
                                        nav_date: None,
                                        source: Some(TENCENT_PRICE_SOURCE),
                                    },
                                )?;
                                ensure_transaction(conn, || {
                                    upsert_market_price(
                                        conn,
                                        &MarketPriceWrite {
                                            instrument_id: &instrument_id,
                                            price_cents,
                                            currency_code: &currency,
                                            priced_at: &ledger_infra::db::now_iso(),
                                            nav_date: None,
                                            source: Some(TENCENT_PRICE_SOURCE),
                                        },
                                    )?;
                                    let trade_date = price_date.clone().unwrap_or_else(|| {
                                        beijing_today().format("%Y-%m-%d").to_string()
                                    });
                                    let day = NaiveDate::parse_from_str(&trade_date, "%Y-%m-%d")
                                        .map_err(|e| {
                                            AppError::Parse(format!("非法交易日 {trade_date}: {e}"))
                                        })?;
                                    if backfill::has_any_history(conn, &instrument_id)? {
                                        commit_price_history_weekly(
                                            conn,
                                            &instrument_id,
                                            &currency,
                                            TENCENT_PRICE_SOURCE,
                                            &[(day, price_cents_to_value(price_cents))],
                                        )?;
                                    }
                                    Ok(())
                                })?;
                                mark(&sink, StockMark::Priced);
                            }
                            Ok(())
                        }
                    })
                    .await;
                for mark in marks
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .drain(..)
                {
                    match mark {
                        StockMark::Renamed => {
                            renamed += 1;
                            witness.mark_written();
                        }
                        StockMark::Priced => {
                            synced_codes.insert(item.code.clone());
                            witness.mark_written();
                        }
                    }
                }
                renamed_now?;
            }
        }
    }

    for _ in &queryable {
        done += 1;
        progress(SyncProgress::instrument(done, total));
    }

    let mut bulk_gaps = 0usize;
    let fund_codes: Vec<String> = funds.iter().map(|fund| fund.symbol.clone()).collect();
    let bulk_data = if funds.is_empty() {
        BulkData::none()
    } else {
        fetch_bulk_surfaces(bulk, &fund_codes).await
    };

    let mut fund_stats = FundSyncStats {
        synced: 0,
        skipped: 0,
        written: 0,
    };
    for fund in &funds {
        let code = fund.symbol.clone();
        let covered = bulk_data.covers(&code);
        let latest_hint = bulk_data.nav_of(&code);
        if bulk_data.has_batch() && !covered {
            bulk_gaps += 1;
            tracing::debug!(
                code = %code,
                "场外基金批量面未收录该标的（新成立 / 已终止 / 清盘），逐只通道补齐"
            );
        }
        {
            let written_before = fund_stats.written;
            refresh_one_fund_price(
                session,
                &FundIdentity::from_instrument(fund),
                latest_hint,
                fetch_nav_history,
                confirm_money_fund,
                &mut fund_stats,
            )
            .await?;
            if fund_stats.written > written_before {
                witness.mark_written();
            }
        }
        let name = match bulk_data.name_of(&code) {
            Some(name) => name.to_string(),
            None => {
                if bulk_data.has_batch() {
                    tracing::debug!(code = %code, "场外基金批量面未收录该标的，逐只名称通道补齐");
                }
                match fetch_fund_name(&fund.symbol).await {
                    Ok(name) => name,
                    Err(error) if is_fund_not_found(&error) => {
                        tracing::warn!(
                            code = %fund.symbol,
                            "基金名称刷新查无此码（搜索索引与档案通道皆未命中），保留原名称"
                        );
                        String::new()
                    }
                    Err(error) => return Err(error),
                }
            }
        };
        let instrument_id = fund.instrument_id.clone();
        if session
            .with_connection(move |conn| refresh_instrument_name(conn, &instrument_id, &name))
            .await?
        {
            renamed += 1;
            witness.mark_written();
        }
        done += 1;
        progress(SyncProgress::instrument(done, total));
    }

    let synced = synced_codes.len() + fund_stats.synced;
    let invalid = queryable.len() - synced_codes.len();
    let skipped = uncollected + invalid + fund_stats.skipped;
    let written = synced_codes.len() + fund_stats.written;
    if bulk_data.degraded || bulk_gaps > 0 {
        tracing::info!(
            bulk_degraded = bulk_data.degraded,
            bulk_gaps,
            "行情批量取数面统计：缺口逐条回退，失败整体降级"
        );
    }

    Ok(SyncInstrumentInfoResult {
        synced,
        skipped,
        written,
        renamed,
        bulk_degraded: bulk_data.degraded,
        bulk_gaps,
        message: format!("已同步 {synced} 只，跳过 {skipped} 只"),
    })
}

pub(crate) fn beijing_today() -> NaiveDate {
    beijing_date(chrono::Utc::now())
}

pub(super) fn beijing_date(now: chrono::DateTime<chrono::Utc>) -> NaiveDate {
    (now + chrono::Duration::hours(8)).date_naive()
}

pub(crate) fn daily_window_opens(last_run: Option<NaiveDate>, today: NaiveDate) -> bool {
    last_run != Some(today)
}

pub(super) fn two_years_ago(today: NaiveDate) -> NaiveDate {
    today
        .checked_sub_months(chrono::Months::new(24))
        .unwrap_or(today)
}

enum StockMark {
    Renamed,
    Priced,
}

fn mark(sink: &Mutex<Vec<StockMark>>, mark: StockMark) {
    sink.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(mark);
}
