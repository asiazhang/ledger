//! 同步进度事件推送（issue #89 / #104 / #369）：进度载荷经主线程非阻塞投递。

use tauri::{AppHandle, Emitter};

use super::model::SyncProgress;
use crate::events;

/// 进度事件名：行情同步进度推送（issue #89 / #104，带 payload，非失效信号）。
/// 前端 `useInstrumentFullSync` 订阅（终态 `done=true` 落定同步状态），
/// 事件名属前后端契约，改名须双侧同步。
pub(super) const SYNC_INSTRUMENTS_PROGRESS: &str = "sync-instruments:progress";

/// 推送同步进度事件（带 payload）：经 [`events::post_emit_with`] 投递主线程
/// 非阻塞执行（issue #369，与失效信号共用同一投递机制、不另起第二套）——
/// 同步线程入队即返回，不持 `webviews_lock` 等主线程回执；投递/发射失败
/// 静默忽略，不影响同步结果；同步线程顺序入队，进度事件在主线程按序执行。
/// 全模块所有 `sync-instruments:progress` 发射点（每页进度 / 错误进度 /
/// 终端进度）唯一收敛于此，不得在同步线程就地 `app.emit`。
pub(super) fn emit_progress(app: &AppHandle, progress: SyncProgress) {
    let handle = app.clone();
    events::post_emit_with(app, move || {
        let _ = handle.emit(SYNC_INSTRUMENTS_PROGRESS, progress);
    });
}

/// 推送同步失败进度事件（done=true + error），供命令外壳与编排共用。
/// 经 [`emit_progress`] 投递主线程非阻塞执行（issue #369）。
pub(crate) fn emit_error_progress(app: &AppHandle, error: String) {
    emit_progress(
        app,
        SyncProgress {
            current: 0,
            total: 0,
            market: String::new(),
            done: true,
            total_inserted: 0,
            total_updated: 0,
            error: Some(error),
            cancelled: false,
        },
    );
}
