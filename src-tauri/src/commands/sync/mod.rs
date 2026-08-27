//! 行情同步（issue #89）：命令外壳 + HTTP 网络层 + 持久化 + 编排。
//!
//! 目录组织：
//! - `http`：HTTP 请求（含多主机切换、重试、限流冷却）与响应解析，可独立测试；
//! - `persist`：`instruments` / `market_prices` 持久化；
//! - `orchestrate`：全量同步编排——市场分页遍历、进度事件推送、新增/更新汇总；
//! - `incremental`：增量同步编排（issue #103）——从当前持仓收集股票批量刷价格；
//! - `tests`：原内嵌测试外迁。
//!
//! 对外暴露两个命令（`commands/mod.rs` 经 `pub use sync::*` 重导出）：
//! `sync_instruments` 全量同步（修标的字典）与 `sync_holding_prices` 增量同步（只刷价格）。

mod http;
mod incremental;
mod orchestrate;
mod persist;
#[cfg(test)]
mod tests;

use std::thread;

use tauri::{AppHandle, Emitter, State};

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{SyncHoldingPricesResult, SyncProgress};

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

/// IPC 命令：同步持仓价格（增量同步，issue #103）。仅刷新当前持仓股票的最新价，
/// 不增删、不改标的字典；无持仓返回明确提示而非报错。异步执行（后台线程池），
/// 不阻塞主线程；返回结果统计（同步 N 只 / 跳过 M 只），前端据此轻量提示。
#[tauri::command]
pub async fn sync_holding_prices(db: State<'_, DbState>) -> Result<SyncHoldingPricesResult> {
    let conn = db.conn.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 命令在后台线程池执行，手包 span 维持 SQL 耗时归因（lib.rs 异步命令归因约定）。
        let span = tracing::info_span!("command", command = "sync_holding_prices");
        let _entered = span.enter();
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        incremental::do_incremental_sync(&conn)
    })
    .await
    .map_err(|e| AppError::Io(format!("同步任务执行失败: {e}")))?
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
