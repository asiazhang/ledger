//! 轮次在途互斥单点（issue #1339 / ADR-0120 决策 3）：同端同库任一时刻至多
//! 一轮在途——轮次出锁后顺序性的承接者（不再借连接锁串行）。
//!
//! - **粒度按连接身份分键**（[`super::scheduler`] 的轮次身份键）：同键（同端
//!   同库）轮次互斥，异键（不同库）并行——三个触发源（手动、打开即同步、
//!   低频轮询 + 写后触发）彼此独立，此前只靠连接锁串行，出锁后由本单点承接。
//! - **manifest 读-改-写独占**：至多一轮在途使 manifest 的读-改-写（含段命名
//!   与位点推进序列）结构性不丢更新；通道层既有的「并发整体替换由下一轮归并
//!   自愈」保留为纵深兜底（ADR-0120 决策 3），不因本单点撤销。
//! - **手动入口在途不启动第二次**：重复触发等待并交出同一轮次报告（复用唯一
//!   在途轮次；回显形态在实施票 #1339 定夺——取「等待并交出同一轮次报告」，
//!   与 ADR-0095 前端「进行中重复触发复用唯一那次在途同步」的既有口径同款，
//!   且不改变 `sync_now` 的 wire 契约：仍返回一个轮次报告）。自动入口在途即
//!   放弃本轮（静默，与「拿不到连接锁就跳过本轮」同口径）。
//! - **异常退出兜底**：轮次执行体经 [`RoundHandle`] 交出结果（含失败）；句柄
//!   释放即在途登记清空，未交出结果先释放（panic 等异常退出）时以程序缺陷
//!   错误兜底完成，等待者不悬挂（fail loud，不静默）。

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use tracing::warn;

use ledger_infra::error::{AppError, Result};

use crate::channel::SyncRoundReport;

/// 在途轮次槽：结果一次写入、全部等待者可读（[`SyncRoundReport`] 是 `Copy`）。
#[derive(Default)]
pub(crate) struct RoundSlot {
    result: Mutex<Option<Result<SyncRoundReport>>>,
    done: Condvar,
}

impl RoundSlot {
    /// 交出轮次结果并唤醒全部等待者（重复完成是程序缺陷：保留首个、告警不静默）。
    fn complete(&self, result: Result<SyncRoundReport>) {
        let mut slot = self.result.lock().unwrap_or_else(|e| e.into_inner());
        if slot.replace(result).is_some() {
            warn!("轮次在途槽被重复完成（程序缺陷），保留首个结果");
        }
        self.done.notify_all();
    }

    /// 等待在途轮次的结果（手动入口复用唯一在途轮次的等待半边）。无限等待是
    /// 安全的：执行体在一切路径（含 panic 兜底）都会交出结果，见 [`RoundHandle`]。
    pub(crate) fn wait(&self) -> Result<SyncRoundReport> {
        let mut slot = self.result.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(result) = slot.as_ref() {
                return result.clone();
            }
            slot = self.done.wait(slot).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn is_completed(&self) -> bool {
        self.result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }
}

/// 在途轮次登记表（进程级单例）：键是轮次身份键（连接互斥体地址，
/// `RoundConn::round_key`），值是在途槽。原位换连在互斥体内换槽、地址不变，
/// 登记对换连透明。
struct RoundGate {
    in_flight: Mutex<HashMap<u64, Arc<RoundSlot>>>,
}

static ROUND_GATE: OnceLock<RoundGate> = OnceLock::new();

fn gate() -> &'static RoundGate {
    ROUND_GATE.get_or_init(|| RoundGate {
        in_flight: Mutex::new(HashMap::new()),
    })
}

/// 轮次登记结果：要么登记成功拿到执行体句柄，要么撞上在途轮次拿到等待句柄。
pub(crate) enum RoundStart {
    /// 登记成功：执行体跑完必须经 [`RoundHandle::complete`] 交出结果（含失败）；
    /// 句柄释放即在途登记清空（先清登记后兜底完成，见 [`RoundHandle::drop`]）。
    Began(RoundHandle),
    /// 已有在途轮次：手动入口等待其结果复用（ADR-0120 决策 3 回显形态），
    /// 自动入口放弃本轮（静默）。
    InFlight(Arc<RoundSlot>),
}

/// 登记一轮在途（同键至多一轮；ADR-0120 决策 3）。
pub(crate) fn begin_round(key: u64) -> RoundStart {
    let mut in_flight = gate().in_flight.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = in_flight.get(&key) {
        return RoundStart::InFlight(Arc::clone(slot));
    }
    let slot = Arc::new(RoundSlot::default());
    in_flight.insert(key, Arc::clone(&slot));
    RoundStart::Began(RoundHandle { key, slot })
}

/// 在途轮次的执行体句柄：交出结果（含失败）后随 drop 清空在途登记。
pub(crate) struct RoundHandle {
    key: u64,
    slot: Arc<RoundSlot>,
}

impl RoundHandle {
    /// 交出轮次结果（成功或失败都算结果）并唤醒等待者；句柄随后释放、在途
    /// 登记清空（新的轮次可立即登记）。
    pub(crate) fn complete(self, result: Result<SyncRoundReport>) {
        self.slot.complete(result);
    }
}

impl Drop for RoundHandle {
    fn drop(&mut self) {
        // 先清在途登记（新轮次可立即登记），再兜底完成（panic 等异常退出时
        // 等待者不悬挂：以程序缺陷错误 fail loud，不静默）。
        let removed = gate()
            .in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
        match removed {
            Some(slot) if Arc::ptr_eq(&slot, &self.slot) => {}
            other => warn!(
                removed_is_self = other.is_some(),
                "轮次在途登记与执行体句柄失配（程序缺陷），跳过登记清理"
            ),
        }
        if !self.slot.is_completed() {
            self.slot.complete(Err(AppError::Invalid(
                "同步轮次执行体异常退出，未交出结果（程序缺陷）".to_string(),
            )));
        }
    }
}
