//! 行情同步（issue #89）：命令外壳 + HTTP 网络层 + 持久化 + 编排。
//!
//! 目录组织：
//! - `http`：HTTP 请求（含多主机切换、重试、限流冷却、Referer）与响应解析（报价
//!   / 日 K / 汇率 K），可独立测试；
//! - `fund`：东财基金详情访问（按代码即拉，issue #301 / ADR-0038）；
//! - `fund_nav`：东财历史净值通道——lsjz 访问、报文解析、水位语义与基金分区
//!   编排（issue #303 / ADR-0038 决策 6）；
//! - `persist`：`instruments` / `market_prices` 持久化 + `price_history` / `fx_rate_history`
//!   周采样 upsert（issue #137）；
//! - `orchestrate`：全量同步编排——市场分页遍历、进度事件推送、新增/更新汇总；
//! - `incremental`：增量同步编排（issue #103，#137 升级，#303 基金分区）——
//!   现价 upsert + 近两年日 K 回填周线落 `price_history` + 汇率 K 线落
//!   `fx_rate_history`（ADR-0019）+ 基金历史净值按水位增量回填（ADR-0038 决策 6）；
//! - `tests`：原内嵌测试外迁。
//!
//! 对外暴露两个命令（`commands/mod.rs` 经 `pub use sync::*` 重导出）：
//! `sync_instruments` 全量同步（修标的字典）与 `sync_holding_prices` 增量同步（只刷价格）。

pub(crate) mod fund;
pub(crate) mod fund_nav;
mod http;
mod incremental;
mod orchestrate;
pub(crate) mod persist;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use tauri::{AppHandle, Emitter, State};

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::events;
use crate::models::{CancelSyncResult, SyncHoldingPricesResult, SyncProgress};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 全量同步中断状态（issue #104）：跨命令共享的运行标志与取消标志（`Arc<AtomicBool>`）。
/// - `running`：当前是否有全量同步在跑（供取消命令判断无同步时的明确行为）。
/// - `cancel_requested`：由取消命令置位，`sync_instruments` 启动时清零；分页循环每页检查。
///
/// 用 `Arc` 以便把标志克隆进后台线程（`sync_instruments` 经 `thread::spawn` 执行）。
pub struct SyncState {
    running: Arc<AtomicBool>,
    cancel_requested: Arc<AtomicBool>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            cancel_requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl SyncState {
    /// 原子地接管一次新同步：仅当无同步在跑（`running` 由 false→true）时才成功，并清除取消标志。
    /// 用 `compare_exchange` 做「单同步在跑」守卫，防止二次启动清掉上一次取消标志或旧线程误清
    /// `running`（issue #104 并发/重入）。返回是否成功接管。
    fn try_start(&self) -> bool {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.cancel_requested.store(false, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 取消标志是否被置位：仅测试据此观察中断切换（生产经分页循环读取原始 `Arc`）。
    #[cfg(test)]
    fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }

    fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
    }

    /// 请求中断：有同步在跑则置位取消并返回 `cancelled=true`，否则无副作用、返回明确提示。
    /// 供 `cancel_sync_instruments` 命令调用；抽出以便测试直接驱动（避免依赖 Tauri State）。
    fn cancel(&self) -> CancelSyncResult {
        if self.is_running() {
            self.request_cancel();
            CancelSyncResult {
                cancelled: true,
                message: "已请求中断同步".into(),
            }
        } else {
            CancelSyncResult {
                cancelled: false,
                message: "当前没有正在进行的同步".into(),
            }
        }
    }
}

/// 进度事件名：行情同步进度推送（issue #89 / #104，带 payload，非失效信号）。
/// 前端 `useInstrumentFullSync` 订阅（终态 `done=true` 落定同步状态），
/// 事件名属前后端契约，改名须双侧同步。
const SYNC_INSTRUMENTS_PROGRESS: &str = "sync-instruments:progress";

/// 推送同步进度事件（带 payload）：经 [`events::post_emit_with`] 投递主线程
/// 非阻塞执行（issue #369，与失效信号共用同一投递机制、不另起第二套）——
/// 同步线程入队即返回，不持 `webviews_lock` 等主线程回执；投递/发射失败
/// 静默忽略，不影响同步结果；同步线程顺序入队，进度事件在主线程按序执行。
/// 全模块所有 `sync-instruments:progress` 发射点（每页进度 / 错误进度 /
/// 终端进度）唯一收敛于此，不得在同步线程就地 `app.emit`。
fn emit_progress(app: &AppHandle, progress: SyncProgress) {
    let handle = app.clone();
    events::post_emit_with(app, move || {
        let _ = handle.emit(SYNC_INSTRUMENTS_PROGRESS, progress);
    });
}

/// 推送同步失败进度事件（done=true + error），供命令外壳与编排共用。
/// 经 [`emit_progress`] 投递主线程非阻塞执行（issue #369）。
fn emit_error_progress(app: &AppHandle, error: String) {
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
        crate::db::write(&conn, incremental::do_incremental_sync)
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

    let cancel = sync_state.cancel_requested.clone();
    let running = sync_state.running.clone();

    thread::spawn(move || {
        let accessor = orchestrate::GlobalConn(conn);
        let result = orchestrate::do_sync(&accessor, &app, &cancel);
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
