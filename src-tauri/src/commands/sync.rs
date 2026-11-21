#![allow(clippy::unreachable)]

use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::shell_support::write_entry::{Outcome, SegmentedFailure, write_entry_segmented_async};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::{WriteEvidence, WriteOp};
use ledger_market_sync::{
    FX_SYNC_GATE, FacadeWriteSession, FxProgressEmitter, FxSyncChannels, FxSyncProgress,
    FxSyncReport, ProgressEmitter, SyncFetchChannels, SyncInstrumentInfoResult, SyncProgress,
    WriteWitness, do_incremental_sync_channels, sync_fx_rates_guarded,
};

pub struct SyncChannelsSlot(pub Arc<tokio::sync::Mutex<SyncFetchChannels>>);

pub(crate) fn facade_session(
    write: ledger_infra::db::DbWriteHandle,
    span: &'static str,
) -> FacadeWriteSession {
    FacadeWriteSession::new(write, span)
}

pub(crate) fn progress_to_emitter(emitter: &dyn ProgressEmitter) -> impl FnMut(SyncProgress) + '_ {
    move |progress| emitter.emit_progress(progress)
}

pub(crate) fn progress_to_fx_emitter(
    emitter: &dyn FxProgressEmitter,
) -> impl FnMut(FxSyncProgress) + '_ {
    move |progress| emitter.emit_fx_progress(progress)
}

#[tauri::command]
pub async fn sync_instrument_info<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
) -> Result<SyncInstrumentInfoResult> {
    let write = db.write_handle();
    let progress_app = app.clone();
    let channels: Arc<tokio::sync::Mutex<SyncFetchChannels>> =
        match app.try_state::<SyncChannelsSlot>() {
            Some(slot) => slot.0.clone(),
            None => Arc::new(tokio::sync::Mutex::new(SyncFetchChannels::production()?)),
        };
    let session_write = write.clone();
    write_entry_segmented_async(
        "sync_instrument_info",
        write,
        Some(&app),
        WriteOp::SyncInstrumentInfo,
        async move {
            let session = facade_session(session_write, "sync_instrument_info");
            let mut progress = progress_to_emitter(&progress_app);
            let mut channels = channels.lock().await;
            let mut witness = WriteWitness::default();
            do_incremental_sync_channels(&session, &mut channels, &mut progress, &mut witness)
                .await
                .map(|result| {
                    let evidence = WriteEvidence::PriceWritten(witness.any_written());
                    Outcome::Evidenced(result, evidence)
                })
                .map_err(|error| SegmentedFailure {
                    error,
                    evidence: WriteEvidence::PriceWritten(witness.any_written()),
                })
        },
    )
    .await
}

#[tauri::command]
pub async fn sync_exchange_rates<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
) -> Result<FxSyncReport> {
    let progress_app = app.clone();
    let session = facade_session(db.write_handle(), "sync_exchange_rates");
    let mut channels = FxSyncChannels::production()?;
    let mut progress = progress_to_fx_emitter(&progress_app);
    sync_fx_rates_guarded(&FX_SYNC_GATE, &session, &mut channels, &mut progress).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_market_sync::ScopedSession;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingEmitter(Mutex<Vec<SyncProgress>>);

    impl ProgressEmitter for RecordingEmitter {
        fn emit_progress(&self, progress: SyncProgress) {
            self.0.lock().unwrap().push(progress);
        }
    }

    #[derive(Default)]
    struct FxRecordingEmitter(Mutex<Vec<FxSyncProgress>>);

    impl FxProgressEmitter for FxRecordingEmitter {
        fn emit_fx_progress(&self, progress: FxSyncProgress) {
            self.0.lock().unwrap().push(progress);
        }
    }

    #[test]
    fn command_shell_hands_write_entry_connection_to_orchestration_via_session() {
        let conn = crate::test_support::open();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(conn));
        let handle = ledger_infra::db::DbSlotPair::new(
            std::sync::Arc::clone(&shared),
            std::sync::Arc::clone(&shared),
        )
        .write_handle();
        let session = facade_session(handle, "sync_instrument_info");

        tauri::async_runtime::block_on(session.with_connection(|c| {
            use ledger_infra::error::AppError;
            let name = "会话";
            c.execute(
                "INSERT INTO categories (id, name, kind, created_at, updated_at, version, device_id) \
                 VALUES ('cat-session', ?1, 'expense', ?2, ?2, 1, 'device-1')",
                rusqlite::params![name, crate::test_support::FIXED_NOW],
            )
            .map_err(AppError::from)?;
            Ok(())
        }))
        .expect("经会话的写入应成功");

        let count: i64 = shared
            .lock()
            .expect("段后锁应可取（短暂作业已还）")
            .query_row(
                "SELECT count(*) FROM categories WHERE id = 'cat-session'",
                [],
                |r| r.get(0),
            )
            .expect("查询应成功");
        assert_eq!(count, 1, "会话交出的应是写入口持有的那把连接");
    }

    #[test]
    fn command_shell_wires_orchestration_progress_to_emitter() {
        let emitter = RecordingEmitter::default();
        let mut progress = progress_to_emitter(&emitter);

        progress(SyncProgress {
            done: 0,
            total: 100,
        });
        progress(SyncProgress {
            done: 37,
            total: 100,
        });
        progress(SyncProgress {
            done: 100,
            total: 100,
        });

        assert_eq!(
            *emitter.0.lock().unwrap(),
            vec![
                SyncProgress {
                    done: 0,
                    total: 100,
                },
                SyncProgress {
                    done: 37,
                    total: 100,
                },
                SyncProgress {
                    done: 100,
                    total: 100,
                },
            ],
            "编排回调逐次直达发射器，载荷形状与顺序保持不变",
        );
    }

    #[test]
    fn command_shell_wires_fx_stage_progress_to_emitter() {
        let emitter = FxRecordingEmitter::default();
        let mut progress = progress_to_fx_emitter(&emitter);

        progress(FxSyncProgress::fetching());
        progress(FxSyncProgress::persisting(21));

        assert_eq!(
            *emitter.0.lock().unwrap(),
            vec![FxSyncProgress::fetching(), FxSyncProgress::persisting(21)],
            "编排阶段回调逐次直达发射器，阶段与天数保持不变"
        );
    }
}
