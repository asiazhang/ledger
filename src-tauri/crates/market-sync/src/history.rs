use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::NaiveDate;
use rusqlite::Connection;
use tauri::{AppHandle, Runtime};

use ledger_infra::error::Result;
use ledger_investment::holdings::FIRST_POSITION_DATE;
use ledger_investment::predicates::INVESTED_EXISTS;
use ledger_investment::{
    InstrumentType, Market, PriceChannel, derive_price_channel, derive_quote_market,
};

use super::channels::{FetchFuture, QuoteQuery, SyncFetchChannels};
use super::fund_backfill::{BackfillOutcome, backfill_one_fund_history};
use super::fund_nav::{FundIdentity, NavPoint};
use super::http::KlineBar;
use super::incremental::{SyncInstrument, beijing_today, quote_code};
use super::lane::{
    LaneChannelsSlot, LaneId, LaneRound, LaneRoundFuture, LaneTimings, progress_forwarder,
    run_background_lane_round, start_daily_lane,
};
use super::model::WriteWitness;
use super::progress::SyncProgress;
use super::session::{FacadeWriteSession, ScopedSession};
use super::weekly::{commit_price_history_weekly, week_monday};
use ledger_investment::backfill;
use ledger_investment::prices::TENCENT_PRICE_SOURCE;

fn week_behind(reference: Option<&str>, today: NaiveDate) -> bool {
    let Some(reference) =
        reference.and_then(|date| NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").ok())
    else {
        return true;
    };
    week_monday(today) - week_monday(reference) > chrono::Duration::days(7)
}

enum BackfillTarget {
    Quote(QuoteQuery),
    FundNav,
}

struct BackfillItem {
    instrument: SyncInstrument,
    target: BackfillTarget,
}

fn collect_backfill_queue(conn: &Connection) -> Result<Vec<BackfillItem>> {
    let today = beijing_today();
    let sql = format!(
        "SELECT i.id, i.symbol, i.market, i.currency_code, i.instrument_type, i.constant_unit_price, \
                MAX(ph.trade_date) AS latest_history, \
                MIN(ph.trade_date) AS earliest_history, \
                MAX(mp.nav_date) AS watermark, \
                {FIRST_POSITION_DATE} AS first_position_date, \
                CASE WHEN {INVESTED_EXISTS} THEN 1 ELSE 0 END AS invested \
         FROM instruments i \
         LEFT JOIN price_history ph ON ph.instrument_id = i.id \
         LEFT JOIN market_prices mp ON mp.instrument_id = i.id \
         GROUP BY i.id \
         ORDER BY invested DESC, i.symbol ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Market>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, InstrumentType>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, i64>(10)? != 0,
        ))
    })?;
    let mut queue = Vec::new();
    for row in rows {
        let (
            instrument_id,
            symbol,
            market,
            currency,
            kind,
            constant_unit_price,
            latest_history,
            earliest_history,
            watermark,
            first_position_date,
            _invested,
        ) = row?;
        let channel = derive_price_channel(kind, market, &symbol, constant_unit_price);
        let quote_market = derive_quote_market(kind, market, &symbol, constant_unit_price);
        let coverage_short = || {
            super::fund_nav::coverage_short_of_first_position(
                earliest_history.as_deref(),
                first_position_date.as_deref(),
            )
        };
        let target = match quote_market {
            Some(quote_market) => {
                let incomplete = match &latest_history {
                    None => true,
                    Some(latest) => week_behind(Some(latest), today) || coverage_short(),
                };
                if !incomplete {
                    continue;
                }
                BackfillTarget::Quote(QuoteQuery {
                    market: quote_market,
                    code: quote_code(&symbol).to_string(),
                })
            }
            None => {
                if channel != PriceChannel::FundNav {
                    continue;
                }
                let incomplete = match &latest_history {
                    None => true,
                    Some(_) => week_behind(watermark.as_deref(), today) || coverage_short(),
                };
                if !incomplete {
                    continue;
                }
                BackfillTarget::FundNav
            }
        };
        queue.push(BackfillItem {
            instrument: SyncInstrument {
                instrument_id,
                symbol,
                market,
                currency,
                kind,
                constant_unit_price,
                channel,
            },
            target,
        });
    }
    Ok(queue)
}

pub(super) async fn backfill_stock_history<Q, K>(
    session: &Q,
    fetch_kline: &mut K,
    query: &QuoteQuery,
    inst: &SyncInstrument,
) -> Result<bool>
where
    Q: ScopedSession,
    K: FnMut(&QuoteQuery) -> FetchFuture<Vec<KlineBar>> + Send,
{
    let first_position: Option<String> = session
        .with_connection({
            let instrument_id = inst.instrument_id.clone();
            move |conn| ledger_investment::holdings::first_position_date(conn, &instrument_id)
        })
        .await?;
    let bars = fetch_kline(query).await?;
    let (start, end) =
        super::fund_nav::deep_backfill_window(first_position.as_deref(), beijing_today());
    let weekly_points: Vec<(NaiveDate, f64)> = bars
        .iter()
        .filter(|bar| bar.date.as_str() >= start.as_str() && bar.date.as_str() <= end.as_str())
        .filter_map(|bar| {
            Some((
                NaiveDate::parse_from_str(&bar.date, "%Y-%m-%d").ok()?,
                bar.close,
            ))
        })
        .collect();
    let (instrument_id, currency) = (inst.instrument_id.clone(), inst.currency.clone());
    session
        .with_connection(move |conn| {
            commit_price_history_weekly(
                conn,
                &instrument_id,
                &currency,
                TENCENT_PRICE_SOURCE,
                &weekly_points,
            )
        })
        .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HistoryBackfillStats {
    pub(super) queued: usize,
    pub(super) failed: usize,
}

pub(super) async fn run_history_backfill_round<Q, K, H, C, P>(
    session: &Q,
    fetch_kline: &mut K,
    fetch_nav_history: &mut H,
    confirm_money_fund: &mut C,
    progress: &mut P,
    witness: &mut WriteWitness,
) -> Result<HistoryBackfillStats>
where
    Q: ScopedSession,
    K: FnMut(&QuoteQuery) -> FetchFuture<Vec<KlineBar>> + Send,
    H: FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send,
    C: FnMut(&str) -> FetchFuture<bool> + Send,
    P: FnMut(SyncProgress) + Send,
{
    let queue = session.with_connection(collect_backfill_queue).await?;
    let total = queue.len();
    if total == 0 {
        return Ok(HistoryBackfillStats {
            queued: 0,
            failed: 0,
        });
    }
    progress(SyncProgress::instrument(0, total));
    backfill::round_started(total);
    let mut done = 0usize;
    let mut failed = 0usize;

    for item in &queue {
        let result: Result<(bool, bool)> = match &item.target {
            BackfillTarget::Quote(query) => {
                backfill_stock_history(session, fetch_kline, query, &item.instrument)
                    .await
                    .map(|written| (written, false))
            }
            BackfillTarget::FundNav => backfill_one_fund_history(
                session,
                &FundIdentity::from_instrument(&item.instrument),
                fetch_nav_history,
                confirm_money_fund,
            )
            .await
            .map(|outcome: BackfillOutcome| (outcome.written, outcome.inconclusive)),
        };
        match &result {
            Ok((written, _)) => {
                if *written {
                    witness.mark_written();
                }
            }
            Err(error) => {
                failed += 1;
                tracing::warn!(
                    instrument = %item.instrument.symbol,
                    %error,
                    "价格历史补全单只失败，继续排空队列（下一窗口按派生事实重进）"
                );
            }
        }
        let attempt_failed = match &result {
            Ok((_, false)) => backfill::BackfillAttempt::NoData,
            Ok((_, true)) | Err(_) => backfill::BackfillAttempt::Failed,
        };
        let instrument_id = item.instrument.instrument_id.clone();
        match session
            .with_connection(move |conn| {
                ledger_investment::backfill::has_any_history(conn, &instrument_id)
            })
            .await
        {
            Ok(false) => backfill::record_attempt(&item.instrument.instrument_id, attempt_failed),
            Ok(true) => {}
            Err(error) => tracing::warn!(
                instrument = %item.instrument.symbol, %error,
                "补全后历史存在性检查失败，三态判据暂按补全中处理"
            ),
        }
        done += 1;
        backfill::round_progress(done);
        progress(SyncProgress::instrument(done, total));
    }
    backfill::round_finished();

    Ok(HistoryBackfillStats {
        queued: total,
        failed,
    })
}

pub struct BackfillChannelsSlot(pub Arc<tokio::sync::Mutex<SyncFetchChannels>>);

impl LaneChannelsSlot for BackfillChannelsSlot {
    fn shared(&self) -> Arc<tokio::sync::Mutex<SyncFetchChannels>> {
        self.0.clone()
    }
}

pub fn start_history_backfill<R: Runtime>(app: &AppHandle<R>) {
    start_history_backfill_with(app, LaneTimings::default());
}

pub fn start_history_backfill_with<R: Runtime>(app: &AppHandle<R>, timings: LaneTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    start_daily_lane(app, &SPAWNED, timings, |handle| {
        run_backfill_round_gated(handle)
    });
}

struct HistoryBackfillRound;

impl LaneRound for HistoryBackfillRound {
    type Stats = HistoryBackfillStats;

    fn run<'a>(
        &'a self,
        session: &'a FacadeWriteSession,
        channels: &'a mut SyncFetchChannels,
        progress: &'a mut (dyn FnMut(SyncProgress) + Send),
        witness: &'a mut WriteWitness,
    ) -> LaneRoundFuture<'a, Self::Stats> {
        Box::pin(async move {
            let SyncFetchChannels {
                fetch_kline,
                fetch_nav_history,
                confirm_money_fund_form,
                ..
            } = channels;
            let mut forward = progress_forwarder(progress);
            run_history_backfill_round(
                session,
                fetch_kline,
                fetch_nav_history,
                confirm_money_fund_form,
                &mut forward,
                witness,
            )
            .await
        })
    }
}

async fn run_backfill_round_gated<R: Runtime>(app: AppHandle<R>) {
    let result = run_background_lane_round::<R, BackfillChannelsSlot, _>(
        &app,
        LaneId::HistoryBackfill,
        HistoryBackfillRound,
    )
    .await;
    match result {
        Ok(stats) if stats.queued == 0 => tracing::debug!("历史补全队列空，本轮零动作"),
        Ok(stats) => tracing::info!(
            queued = stats.queued,
            failed = stats.failed,
            "价格历史后台补全一轮完成"
        ),
        Err(_) => {}
    }
}
