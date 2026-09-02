//! 全量同步中断状态（issue #104）：跨命令共享的运行标志与取消标志。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::model::CancelSyncResult;

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
    pub(crate) fn try_start(&self) -> bool {
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

    /// 克隆两标志供后台线程所有权转移：`(cancel_requested, running)`。
    /// 壳层 `sync_instruments` 启动线程时取用；分页循环读取消标志、线程收尾清运行标志。
    pub(crate) fn flags(&self) -> (Arc<AtomicBool>, Arc<AtomicBool>) {
        (self.cancel_requested.clone(), self.running.clone())
    }

    /// 是否有全量同步在跑：`cancel` 内部判定与测试断言用（生产取消命令经
    /// [`cancel`](Self::cancel) 的返回值观察同一语义）。
    pub(super) fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 取消标志是否被置位：仅测试据此观察中断切换（生产经分页循环读取原始 `Arc`）。
    #[cfg(test)]
    pub(super) fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }

    /// 置位取消标志：`cancel` 与测试驱动共用。
    pub(super) fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
    }

    /// 请求中断：有同步在跑则置位取消并返回 `cancelled=true`，否则无副作用、返回明确提示。
    /// 供 `cancel_sync_instruments` 命令调用；抽出以便测试直接驱动（避免依赖 Tauri State）。
    pub(crate) fn cancel(&self) -> CancelSyncResult {
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
