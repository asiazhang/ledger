use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use tracing::warn;

use ledger_infra::error::{AppError, Result};

use crate::channel::SyncRoundReport;

#[derive(Default)]
pub(crate) struct RoundSlot {
    result: Mutex<Option<Result<SyncRoundReport>>>,
    done: Condvar,
}

impl RoundSlot {
    fn complete(&self, result: Result<SyncRoundReport>) {
        let mut slot = self.result.lock().unwrap_or_else(|e| e.into_inner());
        if slot.replace(result).is_some() {
            warn!("轮次在途槽被重复完成（程序缺陷），保留首个结果");
        }
        self.done.notify_all();
    }

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

struct RoundGate {
    in_flight: Mutex<HashMap<u64, Arc<RoundSlot>>>,
}

static ROUND_GATE: OnceLock<RoundGate> = OnceLock::new();

fn gate() -> &'static RoundGate {
    ROUND_GATE.get_or_init(|| RoundGate {
        in_flight: Mutex::new(HashMap::new()),
    })
}

pub(crate) enum RoundStart {
    Began(RoundHandle),
    InFlight(Arc<RoundSlot>),
}

pub(crate) fn begin_round(key: u64) -> RoundStart {
    let mut in_flight = gate().in_flight.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = in_flight.get(&key) {
        return RoundStart::InFlight(Arc::clone(slot));
    }
    let slot = Arc::new(RoundSlot::default());
    in_flight.insert(key, Arc::clone(&slot));
    RoundStart::Began(RoundHandle { key, slot })
}

pub(crate) struct RoundHandle {
    key: u64,
    slot: Arc<RoundSlot>,
}

impl RoundHandle {
    pub(crate) fn complete(self, result: Result<SyncRoundReport>) {
        self.slot.complete(result);
    }
}

impl Drop for RoundHandle {
    fn drop(&mut self) {
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
