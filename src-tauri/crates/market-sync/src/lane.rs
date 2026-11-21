use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};

use ledger_infra::db::DbState;
use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::error::Result;
use ledger_infra::signals::{WriteEvidence, WriteOp, emit_for};

use super::channels::SyncFetchChannels;
use super::incremental::{beijing_today, daily_window_opens};
use super::model::WriteWitness;
use super::progress::{BackfillProgressEmitter, ProgressEmitter, SyncProgress};
use super::session::FacadeWriteSession;

pub(super) type LaneRoundFuture<'a, Stats> =
    Pin<Box<dyn Future<Output = Result<Stats>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LaneId {
    DailyPriceRefresh,
    HistoryBackfill,
}

impl LaneId {
    fn span(self) -> &'static str {
        match self {
            Self::DailyPriceRefresh => "daily_price_refresh",
            Self::HistoryBackfill => "history_backfill",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::DailyPriceRefresh => "后台每日现价刷新",
            Self::HistoryBackfill => "价格历史后台补全",
        }
    }
}

pub(super) trait LaneChannelsSlot: Send + Sync + 'static {
    fn shared(&self) -> Arc<tokio::sync::Mutex<SyncFetchChannels>>;
}

pub(super) trait LaneRound: Send {
    type Stats;

    fn run<'a>(
        &'a self,
        session: &'a FacadeWriteSession,
        channels: &'a mut SyncFetchChannels,
        progress: &'a mut (dyn FnMut(SyncProgress) + Send),
        witness: &'a mut WriteWitness,
    ) -> LaneRoundFuture<'a, Self::Stats>;
}

pub(super) fn progress_forwarder(
    sink: &mut (dyn FnMut(SyncProgress) + Send),
) -> impl FnMut(SyncProgress) + Send + '_ {
    |update| sink(update)
}

pub(super) async fn run_background_lane_round<R, S, L>(
    app: &AppHandle<R>,
    lane: LaneId,
    round: L,
) -> Result<L::Stats>
where
    R: Runtime,
    S: LaneChannelsSlot,
    L: LaneRound,
{
    let span = lane.span();
    let write = app.state::<DbState>().write_handle();
    let session = FacadeWriteSession::new(write.clone(), span);
    let mut witness = WriteWitness::default();
    let mut progress = |progress: SyncProgress| match lane {
        LaneId::DailyPriceRefresh => ProgressEmitter::emit_progress(app, progress),
        LaneId::HistoryBackfill => app.emit_backfill_progress(progress),
    };
    let result = match app.try_state::<S>() {
        Some(slot) => {
            let shared = slot.shared();
            let mut channels = shared.lock().await;
            round
                .run(&session, &mut channels, &mut progress, &mut witness)
                .await
        }
        None => match SyncFetchChannels::production_backfill() {
            Ok(channels) => {
                let channels = tokio::sync::Mutex::new(channels);
                let mut guard = channels.lock().await;
                round
                    .run(&session, &mut guard, &mut progress, &mut witness)
                    .await
            }
            Err(error) => Err(error),
        },
    };

    if witness.any_written() {
        if let Err(error) = write.run(span, |_| Ok(())).await {
            tracing::warn!(
                %error,
                lane = %span,
                "{}收尾置脏失败（脏标记待下次写入补上）",
                lane.name()
            );
        }
        emit_for(
            app,
            WriteOp::SyncInstrumentInfo,
            WriteEvidence::PriceWritten(true),
        );
    }
    if let Err(error) = &result {
        tracing::warn!(%error, lane = %span, "{}失败（静默等下一窗口）", lane.name());
    }
    result
}

const STARTUP_DELAY: Duration = Duration::from_secs(30);

const WINDOW_POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneTimings {
    pub startup_delay: Duration,
    pub window_poll: Duration,
}

impl Default for LaneTimings {
    fn default() -> Self {
        Self {
            startup_delay: STARTUP_DELAY,
            window_poll: WINDOW_POLL_INTERVAL,
        }
    }
}

pub(super) fn start_daily_lane<R, F, Fut>(
    app: &AppHandle<R>,
    spawned: &'static AtomicBool,
    timings: LaneTimings,
    round: F,
) where
    R: Runtime,
    F: Fn(AppHandle<R>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    if spawned.swap(true, Ordering::SeqCst) {
        return;
    }
    let gate = EncryptionGate::clone(&app.state::<EncryptionGate>());
    let boot_gate = BootFailureGate::clone(&app.state::<BootFailureGate>());
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(timings.startup_delay).await;
        let mut last_round_date: Option<chrono::NaiveDate> = None;
        loop {
            if !gate.is_locked() && !boot_gate.is_failed() {
                let today = beijing_today();
                if daily_window_opens(last_round_date, today) {
                    last_round_date = Some(today);
                    round(handle.clone()).await;
                }
            }
            tokio::time::sleep(timings.window_poll).await;
        }
    });
}
