use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use ledger_infra::events::post_emit_with;

pub const INSTRUMENT_SYNC_PROGRESS: &str = "ledger:instrument-sync-progress";

pub const HISTORY_BACKFILL_PROGRESS: &str = "ledger:history-backfill-progress";

pub const FX_SYNC_PROGRESS: &str = "ledger:fx-sync-progress";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FxSyncStage {
    Fetching,
    Persisting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FxSyncProgress {
    pub stage: FxSyncStage,
    pub days_parsed: Option<usize>,
}

impl FxSyncProgress {
    pub fn fetching() -> Self {
        Self {
            stage: FxSyncStage::Fetching,
            days_parsed: None,
        }
    }

    pub fn persisting(days_parsed: usize) -> Self {
        Self {
            stage: FxSyncStage::Persisting,
            days_parsed: Some(days_parsed),
        }
    }
}

pub trait FxProgressEmitter: Send + Sync {
    fn emit_fx_progress(&self, progress: FxSyncProgress);
}

impl<R: Runtime> FxProgressEmitter for AppHandle<R> {
    fn emit_fx_progress(&self, progress: FxSyncProgress) {
        let handle = self.clone();
        post_emit_with(self, move || {
            let _ = handle.emit(FX_SYNC_PROGRESS, progress);
        });
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncProgress {
    pub done: usize,
    pub total: usize,
}

impl SyncProgress {
    pub fn instrument(done: usize, total: usize) -> Self {
        Self { done, total }
    }
}

pub trait ProgressEmitter: Send + Sync {
    fn emit_progress(&self, progress: SyncProgress);
}

impl<R: Runtime> ProgressEmitter for AppHandle<R> {
    fn emit_progress(&self, progress: SyncProgress) {
        let handle = self.clone();
        post_emit_with(self, move || {
            let _ = handle.emit(INSTRUMENT_SYNC_PROGRESS, progress);
        });
    }
}

pub trait BackfillProgressEmitter: Send + Sync {
    fn emit_backfill_progress(&self, progress: SyncProgress);
}

impl<R: Runtime> BackfillProgressEmitter for AppHandle<R> {
    fn emit_backfill_progress(&self, progress: SyncProgress) {
        let handle = self.clone();
        post_emit_with(self, move || {
            let _ = handle.emit(HISTORY_BACKFILL_PROGRESS, progress);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_name_lives_in_ledger_namespace() {
        assert_eq!(INSTRUMENT_SYNC_PROGRESS, "ledger:instrument-sync-progress");
        assert!(INSTRUMENT_SYNC_PROGRESS.starts_with("ledger:"));
        assert!(!INSTRUMENT_SYNC_PROGRESS.ends_with("-changed"));
    }

    #[test]
    fn payload_serializes_as_done_total() {
        let json = serde_json::to_value(SyncProgress {
            done: 37,
            total: 100,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "done": 37, "total": 100 }));
    }

    #[test]
    fn fx_event_name_lives_in_ledger_namespace() {
        assert_eq!(FX_SYNC_PROGRESS, "ledger:fx-sync-progress");
        assert!(FX_SYNC_PROGRESS.starts_with("ledger:"));
        assert!(!FX_SYNC_PROGRESS.ends_with("-changed"));
    }

    #[test]
    fn fx_payload_serializes_as_stage_with_optional_days() {
        let fetching = serde_json::to_value(FxSyncProgress::fetching()).unwrap();
        assert_eq!(
            fetching,
            serde_json::json!({ "stage": "fetching", "days_parsed": null })
        );
        let persisting = serde_json::to_value(FxSyncProgress::persisting(21)).unwrap();
        assert_eq!(
            persisting,
            serde_json::json!({ "stage": "persisting", "days_parsed": 21 })
        );
    }
}
