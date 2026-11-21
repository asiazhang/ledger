use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{Connection, params};
use serde::Serialize;
use tokio::sync::Mutex as AsyncMutex;

use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::default_currency_code;

use super::channels::FetchFuture;
use super::ecb::{
    ECB_HOSTS, EcbDayRates, derive_ecb_weekly_series, fetch_ecb_90d_incremental,
    fetch_ecb_full_history, full_history_cfg,
};
use super::http::{Pacer, RetryConfig, build_client, lock_pacer, shared_pacer};
use super::persist::{ECB_FX_SOURCE, FxPersistReport, persist_ecb_fx_series};
use super::progress::FxSyncProgress;
use super::session::ScopedSession;

pub(crate) const FX_SOURCE_ORIGIN_WEEK: &str = "2005-03-28";

fn source_unreachable() -> AppError {
    AppError::coded(
        "fx.source-unreachable",
        "汇率数据源暂时不可达，请检查网络后重试",
    )
}

fn source_no_data() -> AppError {
    AppError::coded(
        "fx.source-no-data",
        "数据源已连通，但当前没有可用的汇率数据，请稍后重试",
    )
}

fn sync_in_progress() -> AppError {
    AppError::coded("fx.sync-in-progress", "已有汇率同步在进行，请稍后再试")
}

pub struct FxSyncGate {
    in_flight: AtomicBool,
}

impl FxSyncGate {
    pub const fn new() -> Self {
        Self {
            in_flight: AtomicBool::new(false),
        }
    }

    fn try_begin(&self) -> Option<FxSyncGuard<'_>> {
        self.in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| FxSyncGuard { gate: self })
    }
}

impl Default for FxSyncGate {
    fn default() -> Self {
        Self::new()
    }
}

struct FxSyncGuard<'a> {
    gate: &'a FxSyncGate,
}

impl Drop for FxSyncGuard<'_> {
    fn drop(&mut self) {
        self.gate.in_flight.store(false, Ordering::Release);
    }
}

pub static FX_SYNC_GATE: FxSyncGate = FxSyncGate::new();

fn classify_fetch_error(error: AppError) -> AppError {
    if error.is_code("fx.source-malformed") {
        return error;
    }
    tracing::warn!(error = %error, "汇率数据源不可达（取数失败已归类）");
    source_unreachable()
}

fn ensure_series_has_points(series: &[super::ecb::FxPairWeeklySeries]) -> Result<()> {
    if series.iter().all(|s| s.points.is_empty()) {
        return Err(source_no_data());
    }
    Ok(())
}

pub type FetchEcbDocument = Box<dyn FnMut() -> FetchFuture<Vec<EcbDayRates>> + Send>;

pub struct FxSyncChannels {
    pub fetch_full: FetchEcbDocument,
    pub fetch_incremental: FetchEcbDocument,
}

impl FxSyncChannels {
    pub fn production() -> Result<Self> {
        let hosts = ECB_HOSTS.iter().map(|host| (*host).to_string()).collect();
        Self::with_hosts(hosts)
    }

    pub(super) fn with_hosts(ecb_hosts: Vec<String>) -> Result<Self> {
        Self::with_hosts_on(
            ecb_hosts,
            shared_pacer(),
            full_history_cfg(),
            RetryConfig::production(),
        )
    }

    pub(super) fn with_hosts_on(
        ecb_hosts: Vec<String>,
        pacer: Arc<AsyncMutex<Pacer>>,
        full_cfg: RetryConfig,
        incremental_cfg: RetryConfig,
    ) -> Result<Self> {
        let client = build_client()?;
        Ok(Self {
            fetch_full: {
                let client = client.clone();
                let pacer = pacer.clone();
                let hosts = ecb_hosts.clone();
                Box::new(move || {
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    Box::pin(async move {
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        fetch_ecb_full_history(&client, &mut pacer, &hosts, full_cfg).await
                    }) as FetchFuture<Vec<EcbDayRates>>
                })
            },
            fetch_incremental: {
                let client = client.clone();
                let pacer = pacer.clone();
                let hosts = ecb_hosts.clone();
                Box::new(move || {
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    Box::pin(async move {
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        fetch_ecb_90d_incremental(&client, &mut pacer, &hosts, incremental_cfg)
                            .await
                    }) as FetchFuture<Vec<EcbDayRates>>
                })
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct FxSyncReport {
    pub full_backfilled: bool,
    pub persist: FxPersistReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FxSyncPlan {
    Skip,
    FullBackfill,
    Incremental,
}

pub async fn sync_fx_rates<S: ScopedSession>(
    session: &S,
    channels: &mut FxSyncChannels,
) -> Result<FxSyncReport> {
    sync_fx_rates_with_progress(session, channels, &mut |_| {}).await
}

pub async fn sync_fx_rates_with_progress<S, P>(
    session: &S,
    channels: &mut FxSyncChannels,
    progress: &mut P,
) -> Result<FxSyncReport>
where
    S: ScopedSession,
    P: FnMut(FxSyncProgress) + Send,
{
    let (plan, pairs) = session.with_connection(plan_fx_sync).await?;
    match plan {
        FxSyncPlan::Skip => {
            tracing::debug!("账本无非本位币痕迹，汇率同步跳过（零请求零落库）");
            Ok(FxSyncReport::default())
        }
        FxSyncPlan::FullBackfill => {
            progress(FxSyncProgress::fetching());
            let days = (channels.fetch_full)()
                .await
                .map_err(classify_fetch_error)?;
            let series = derive_ecb_weekly_series(&days, &pairs);
            ensure_series_has_points(&series)?;
            progress(FxSyncProgress::persisting(days.len()));
            let persist = session
                .with_connection(move |conn| persist_ecb_fx_series(conn, &series))
                .await?;
            tracing::info!(
                earliest = persist.earliest.as_deref().unwrap_or("-"),
                points = persist.points,
                "ECB 汇率全量回填完成（整份文件灌库）"
            );
            Ok(FxSyncReport {
                full_backfilled: true,
                persist,
            })
        }
        FxSyncPlan::Incremental => {
            progress(FxSyncProgress::fetching());
            let days = (channels.fetch_incremental)()
                .await
                .map_err(classify_fetch_error)?;
            let series = derive_ecb_weekly_series(&days, &pairs);
            ensure_series_has_points(&series)?;
            progress(FxSyncProgress::persisting(days.len()));
            let persist = session
                .with_connection(move |conn| persist_ecb_fx_series(conn, &series))
                .await?;
            tracing::debug!(
                points = persist.points,
                "ECB 汇率增量同步完成（深度已达成）"
            );
            Ok(FxSyncReport {
                full_backfilled: false,
                persist,
            })
        }
    }
}

pub async fn sync_fx_rates_guarded<S, P>(
    gate: &FxSyncGate,
    session: &S,
    channels: &mut FxSyncChannels,
    progress: &mut P,
) -> Result<FxSyncReport>
where
    S: ScopedSession,
    P: FnMut(FxSyncProgress) + Send,
{
    let _guard = gate.try_begin().ok_or_else(sync_in_progress)?;
    sync_fx_rates_with_progress(session, channels, progress).await
}

fn plan_fx_sync(conn: &Connection) -> Result<(FxSyncPlan, Vec<(String, String)>)> {
    let native = default_currency_code(conn)?;
    let mut pairs: Vec<(String, String)> = {
        let mut stmt =
            conn.prepare("SELECT code FROM currencies WHERE code <> ?1 ORDER BY code")?;
        let codes = stmt
            .query_map(params![native], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        codes
            .into_iter()
            .map(|code| (code, native.clone()))
            .collect()
    };
    pairs.dedup();

    if !has_non_native_trace(conn, &native)? {
        return Ok((FxSyncPlan::Skip, pairs));
    }
    if fx_depth_reached(conn, &native)? {
        Ok((FxSyncPlan::Incremental, pairs))
    } else {
        Ok((FxSyncPlan::FullBackfill, pairs))
    }
}

fn has_non_native_trace(conn: &Connection, native: &str) -> Result<bool> {
    let found: i64 = conn.query_row(
        "SELECT EXISTS( \
             SELECT 1 FROM accounts \
              WHERE is_deleted = 0 AND is_hidden = 0 AND currency_code <> ?1 \
             UNION ALL \
             SELECT 1 FROM transactions \
              WHERE is_deleted = 0 AND currency_code <> ?1 \
         )",
        params![native],
        |row| row.get(0),
    )?;
    Ok(found == 1)
}

fn fx_depth_reached(conn: &Connection, native: &str) -> Result<bool> {
    let missing: i64 = conn.query_row(
        "SELECT count(*) FROM currencies c \
          WHERE c.code <> ?1 \
            AND NOT EXISTS ( \
                SELECT 1 FROM fx_rate_history f \
                 WHERE f.base_code = c.code AND f.quote_code = ?1 \
                   AND f.week_start <= ?2 AND f.source = ?3 \
            )",
        params![native, FX_SOURCE_ORIGIN_WEEK, ECB_FX_SOURCE],
        |row| row.get(0),
    )?;
    Ok(missing == 0)
}
