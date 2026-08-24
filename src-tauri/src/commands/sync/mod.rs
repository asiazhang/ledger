//! 行情同步（issue #89）：命令外壳 + HTTP 网络层 + 持久化 + 编排。
//!
//! 目录组织：
//! - `http`：HTTP 请求（含多主机切换、重试、限流冷却）与响应解析，可独立测试；
//! - `persist`：`instruments` / `market_prices` 持久化；
//! - `orchestrate`：同步编排——市场分页遍历、进度事件推送、新增/更新汇总；
//! - `tests`：原内嵌测试外迁。
//!
//! 对外仅暴露 `sync_instruments` 命令（`commands/mod.rs` 经 `pub use sync::*`
//! 重导出，注册路径 `commands::sync_instruments` 与前端调用零改动）。

mod http;
mod orchestrate;
mod persist;
#[cfg(test)]
mod tests;

use std::thread;

use tauri::{AppHandle, Emitter, State};

use crate::db::DbState;
use crate::error::Result;
use crate::models::SyncProgress;

/// 推送同步失败进度事件（done=true + error），供命令外壳与编排共用。
fn emit_error_progress(app: &AppHandle, error: String) {
    let _ = app.emit(
        "sync-instruments:progress",
        SyncProgress {
            current: 0,
            total: 0,
            market: String::new(),
            done: true,
            total_inserted: 0,
            total_updated: 0,
            error: Some(error),
        },
    );
}

/// IPC 命令：全量同步股票行情。后台线程执行（不阻塞界面），进度经
/// `sync-instruments:progress` 事件推送，完成/失败时 `done=true` 并携带结果或错误。
#[tauri::command]
pub fn sync_instruments(db: State<'_, DbState>, app: tauri::AppHandle) -> Result<()> {
    let conn = db.conn.clone();

    thread::spawn(move || {
        let conn_guard = match conn.lock() {
            Ok(g) => g,
            Err(e) => {
                emit_error_progress(&app, format!("数据库锁定失败: {e}"));
                return;
            }
        };

        if let Err(e) = orchestrate::do_sync(&conn_guard, &app) {
            tracing::error!(error = %e, "股票同步失败");
            emit_error_progress(&app, e.to_string());
        }
    });

    Ok(())
}
