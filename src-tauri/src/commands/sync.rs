//! 行情同步 IPC 命令壳（issue #89；#407 域目录化后压平为单文件纯壳）。
//!
//! 只做参数解包、事务壳与信号发射；行情同步引擎（HTTP 网络层、东财基金访问、
//! 增全量同步编排、同步状态与进度推送）在 [`crate::sync`] 域目录。对外暴露
//! `sync_holding_prices` / `sync_instruments` / `cancel_sync_instruments` 三个
//! 命令（`commands/mod.rs` 经 `pub use sync::*` 重导出，注册路径与前端/BDD
//! 调用零改动）。

use std::sync::atomic::Ordering;
use std::thread;

use tauri::{AppHandle, State};

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::signals::{WriteEvidence, WriteOp, emit_for};
use crate::sync::{
    CancelSyncResult, GlobalConn, SyncHoldingPricesResult, SyncState, do_incremental_sync, do_sync,
    emit_error_progress,
};

/// IPC 命令：同步持仓价格（增量同步，issue #103 / #303）。单次按类型分区刷价：
/// 股票走行情报价/K 线通道，场外基金走历史净值通道逐只按水位增量回填
///（ADR-0038 决策 6）；不增删、不改标的字典；无持仓返回明确提示而非报错。
/// 异步执行（后台线程池），不阻塞主线程；返回结果统计（同步 N 只 / 跳过 M 只），
/// 前端据此轻量提示。成功且实际写入价格（`written > 0`）时发价格失效信号
///（ADR-0031）：零变化（无持仓/全部跳过/基金无新净值）为库内零变化，不广播。
///
/// 「是否发」判定已于 #333 归一化进 signals 映射单点（`signals_for` +
/// [`WriteEvidence::PriceWritten`]，ADR-0044）：壳层只把终态归一化为证据——
/// 到达保留落库的终态（成功或用户中断）按实际写入 n>0，失败无证据零信号。
#[tauri::command]
pub async fn sync_holding_prices(
    db: State<'_, DbState>,
    app: AppHandle,
) -> Result<SyncHoldingPricesResult> {
    let conn = db.conn.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        // 命令在后台线程池执行，手包 span 维持 SQL 耗时归因（lib.rs 异步命令归因约定）。
        let span = tracing::info_span!("command", command = "sync_holding_prices");
        let _entered = span.enter();
        // 连接层统一写入口（ADR-0032，#246 审计补齐）：行情/汇率/历史落库成功即置脏，
        // 提交点写时顺带到期检查；锁语义与先前整段持有一致（同步期间独占连接）。
        crate::db::write(&conn, do_incremental_sync)
    })
    .await
    .map_err(|e| AppError::Io(format!("同步任务执行失败: {e}")))?;
    // 失败（Err）无证据零信号不广播；成功路径把实际写入归一化为证据（零变化
    // 不广播：基金全部「已是最新」时 written 为 0，虽有成功处理亦不广播），
    // 「是否发」单点在 signals 映射（ADR-0044，#333），壳层只归一化证据并转发。
    if let Ok(r) = &result {
        emit_for(
            &app,
            WriteOp::SyncHoldingPrices,
            WriteEvidence::PriceWritten(r.written > 0),
        );
    }
    result
}

/// IPC 命令：全量同步股票行情。后台线程执行（不阻塞界面），进度经
/// `sync-instruments:progress` 事件推送，完成/失败/中断时 `done=true` 并携带结果、错误或中断标记。
/// 启动时清除取消标志并标记运行中；分页循环每页检查取消标志（issue #104）。
/// 连接经访问器按页短暂获取/释放（issue #147）：网络拉取与进度推送不持锁，
/// 同步期间其它命令可正常执行，锁失败随结果以错误事件上报。
#[tauri::command]
pub fn sync_instruments(
    db: State<'_, DbState>,
    sync_state: State<'_, SyncState>,
    app: tauri::AppHandle,
) -> Result<()> {
    let conn = db.conn.clone();

    // 原子接管起点：仅当无同步在跑才启动；防止二次启动清掉已被置位的取消标志（issue #104）。
    if !sync_state.try_start() {
        return Err(AppError::coded(
            "sync.already-running",
            "已有全量同步正在进行，请先中断或等待完成",
        ));
    }

    let (cancel, running) = sync_state.flags();

    thread::spawn(move || {
        let accessor = GlobalConn(conn);
        let result = do_sync(&accessor, &app, &cancel);
        running.store(false, Ordering::SeqCst);

        match result {
            Ok(outcome) => {
                // 结束（成功或用户中断）且本次运行有落库即发价格失效信号（ADR-0031）：
                // 中断保留已落库价格（upsert 幂等），不发信号即失真；零落库不广播。
                // 「是否发」单点在 signals 映射（ADR-0044，#333），壳层只归一化证据。
                emit_for(
                    &app,
                    WriteOp::SyncInstruments,
                    WriteEvidence::PriceWritten(outcome.written() > 0),
                );
            }
            Err(e) => {
                // 失败不广播（ADR-0031）：emit 的终态只有成功与用户中断，失败不在其列。
                tracing::error!(error = %e, "股票同步失败");
                emit_error_progress(&app, e.to_string());
            }
        }
    });

    Ok(())
}

/// IPC 命令：请求中断正在进行的全量同步（issue #104）。置位取消标志，分页循环下一页命中
/// 即提前返回并推送中断态事件；已落库数据保留，下次重跑自动续上。无同步进行时无副作用。
/// 返回 [`CancelSyncResult`]，供前端据此提示是否真的中断了同步。
#[tauri::command]
pub fn cancel_sync_instruments(sync_state: State<'_, SyncState>) -> CancelSyncResult {
    sync_state.cancel()
}
