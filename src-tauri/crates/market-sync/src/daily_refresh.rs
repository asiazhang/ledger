use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tauri::{AppHandle, Runtime};

use super::channels::SyncFetchChannels;
use super::channels::do_incremental_sync_channels;
use super::lane::{
    LaneChannelsSlot, LaneId, LaneRound, LaneRoundFuture, LaneTimings, progress_forwarder,
    run_background_lane_round, start_daily_lane,
};
use super::model::{SyncInstrumentInfoResult, WriteWitness};
use super::progress::SyncProgress;
use super::session::FacadeWriteSession;

pub struct DailyPriceRefreshChannelsSlot(pub Arc<tokio::sync::Mutex<SyncFetchChannels>>);

impl LaneChannelsSlot for DailyPriceRefreshChannelsSlot {
    fn shared(&self) -> Arc<tokio::sync::Mutex<SyncFetchChannels>> {
        self.0.clone()
    }
}

pub fn start_daily_price_refresh<R: Runtime>(app: &AppHandle<R>) {
    start_daily_price_refresh_with(app, LaneTimings::default());
}

pub fn start_daily_price_refresh_with<R: Runtime>(app: &AppHandle<R>, timings: LaneTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    start_daily_lane(app, &SPAWNED, timings, |handle| {
        run_daily_refresh_round(handle)
    });
}

struct DailyRefreshRound;

impl LaneRound for DailyRefreshRound {
    type Stats = SyncInstrumentInfoResult;

    fn run<'a>(
        &'a self,
        session: &'a FacadeWriteSession,
        channels: &'a mut SyncFetchChannels,
        progress: &'a mut (dyn FnMut(SyncProgress) + Send),
        witness: &'a mut WriteWitness,
    ) -> LaneRoundFuture<'a, Self::Stats> {
        Box::pin(async move {
            let mut forward = progress_forwarder(progress);
            do_incremental_sync_channels(session, channels, &mut forward, witness).await
        })
    }
}

async fn run_daily_refresh_round<R: Runtime>(app: AppHandle<R>) {
    let result = run_background_lane_round::<R, DailyPriceRefreshChannelsSlot, _>(
        &app,
        LaneId::DailyPriceRefresh,
        DailyRefreshRound,
    )
    .await;
    match result {
        Ok(stats) if stats.written == 0 && stats.renamed == 0 => {
            tracing::debug!("每日现价刷新完成：全部已是最新，零写入");
        }
        Ok(stats) => tracing::info!(
            synced = stats.synced,
            written = stats.written,
            renamed = stats.renamed,
            skipped = stats.skipped,
            "后台每日现价刷新完成"
        ),
        Err(_) => {}
    }
}
