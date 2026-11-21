use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tauri::{AppHandle, Manager, Runtime};

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::fx::{FX_SYNC_GATE, FxSyncChannels, FxSyncReport, sync_fx_rates_guarded};
use super::lane::{LaneTimings, start_daily_lane};
use super::session::FacadeWriteSession;

pub struct DailyFxSyncChannelsSlot(pub Arc<tokio::sync::Mutex<FxSyncChannels>>);

pub fn start_daily_fx_sync<R: Runtime>(app: &AppHandle<R>) {
    start_daily_fx_sync_with(app, LaneTimings::default());
}

pub fn start_daily_fx_sync_with<R: Runtime>(app: &AppHandle<R>, timings: LaneTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    start_daily_lane(app, &SPAWNED, timings, |handle| {
        run_daily_fx_sync_round(handle)
    });
}

async fn run_daily_fx_sync_round<R: Runtime>(app: AppHandle<R>) {
    let result = match app.try_state::<DailyFxSyncChannelsSlot>() {
        Some(slot) => {
            let shared = slot.0.clone();
            let mut channels = shared.lock().await;
            drive_round(&app, &mut channels).await
        }
        None => match FxSyncChannels::production() {
            Ok(mut channels) => drive_round(&app, &mut channels).await,
            Err(error) => Err(error),
        },
    };
    match result {
        Ok(report) if report.persist.points == 0 => {
            tracing::debug!("每日汇率同步完成：零落库点（零痕迹跳过或数据已是最新）");
        }
        Ok(report) => tracing::info!(
            full_backfilled = report.full_backfilled,
            pairs = report.persist.pairs,
            points = report.persist.points,
            earliest = report.persist.earliest.as_deref().unwrap_or("-"),
            manual_protected = report.persist.manual_protected,
            "后台每日汇率同步完成"
        ),
        Err(error) => {
            tracing::warn!(%error, "后台每日汇率同步失败（已有汇率不受影响，静默等下一自然日窗口）")
        }
    }
}

async fn drive_round<R: Runtime>(
    app: &AppHandle<R>,
    channels: &mut FxSyncChannels,
) -> Result<FxSyncReport> {
    let write = app.state::<DbState>().write_handle();
    let session = FacadeWriteSession::new(write, "daily_fx_sync");
    sync_fx_rates_guarded(&FX_SYNC_GATE, &session, channels, &mut |_| {}).await
}
