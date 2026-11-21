use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use rusqlite::Connection;

use super::connection::open_in_memory;
use super::migrate::init_db;
use crate::error::{AppError, Result};

pub const LOCK_HOLD_PROBE_THRESHOLD: Duration = Duration::from_secs(1);

pub fn probe_lock_hold_within(hold: Duration, threshold: Duration) {
    if hold >= threshold {
        tracing::warn!(
            hold_ms = hold.as_millis() as u64,
            threshold_ms = threshold.as_millis() as u64,
            "连接锁持有时长超过阈值（疑似慢闭包进锁，ADR-0069 决策 4：分钟级网络往返不得进锁）"
        );
    }
}

pub fn probe_lock_hold(hold: Duration) {
    probe_lock_hold_within(hold, LOCK_HOLD_PROBE_THRESHOLD);
}

static LOCK_HOLD_BUDGETS: OnceLock<Mutex<Vec<(String, Duration)>>> = OnceLock::new();

pub fn register_lock_hold_budget(command_prefix: impl Into<String>, threshold: Duration) {
    LOCK_HOLD_BUDGETS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((command_prefix.into(), threshold));
}

pub(crate) fn hold_threshold_for(command: &str) -> Duration {
    let Some(budgets) = LOCK_HOLD_BUDGETS.get() else {
        return LOCK_HOLD_PROBE_THRESHOLD;
    };
    budgets
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|(prefix, _)| command.starts_with(prefix.as_str()))
        .map(|(_, threshold)| *threshold)
        .max()
        .unwrap_or(LOCK_HOLD_PROBE_THRESHOLD)
}

pub fn write_locked<T>(conn: &Connection, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let result = f(conn);
    if result.is_ok() && conn.is_autocommit() {
        after_commit(conn);
    }
    result
}

pub type AfterCommitHook = fn(&Connection);

static AFTER_COMMIT_HOOK: OnceLock<AfterCommitHook> = OnceLock::new();

pub fn register_after_commit_hook(hook: AfterCommitHook) {
    let _ = AFTER_COMMIT_HOOK.set(hook);
}

fn after_commit(conn: &Connection) {
    match AFTER_COMMIT_HOOK.get() {
        Some(hook) => hook(conn),
        None => tracing::error!(
            "db 提交点后置动作未注册：写已提交但置脏/到期检查被跳过（壳层启动接线缺失）"
        ),
    }
}

pub async fn run_db<T, F>(command: &'static str, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let dispatch = tracing::dispatcher::get_default(tracing::Dispatch::clone);
    let caller_span = tracing::Span::current();
    tauri::async_runtime::spawn_blocking(move || {
        with_caller_context(&dispatch, &caller_span, command, f)
    })
    .await
    .map_err(|e| AppError::Io(format!("数据库任务执行失败: {e}")))?
}

pub(crate) fn with_caller_context<R>(
    dispatch: &tracing::Dispatch,
    caller_span: &tracing::Span,
    command: &'static str,
    f: impl FnOnce() -> R,
) -> R {
    tracing::dispatcher::with_default(dispatch, || {
        let _caller = caller_span.enter();
        let command_span = tracing::info_span!("command", command);
        let _command = caller_span.is_none().then(|| command_span.enter());
        f()
    })
}

static SLOT_SWAP_EPOCHS: OnceLock<Mutex<HashMap<usize, Weak<AtomicU64>>>> = OnceLock::new();

fn slot_swap_epoch_handle(slot: &Arc<Mutex<Connection>>) -> Arc<AtomicU64> {
    let key = Arc::as_ptr(slot) as usize;
    let mut table = SLOT_SWAP_EPOCHS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = table.get(&key).and_then(Weak::upgrade) {
        return existing;
    }
    table.retain(|_, weak| weak.strong_count() > 0);
    let epoch = Arc::new(AtomicU64::new(0));
    table.insert(key, Arc::downgrade(&epoch));
    epoch
}

fn bump_slot_swap_epoch(slot: &Arc<Mutex<Connection>>) {
    slot_swap_epoch_handle(slot).fetch_add(1, Ordering::SeqCst);
}

pub(crate) struct SlotWatch {
    slot: Arc<Mutex<Connection>>,
    epoch: Arc<AtomicU64>,
    seen: std::cell::Cell<u64>,
}

impl SlotWatch {
    pub(crate) fn new(slot: &Arc<Mutex<Connection>>) -> Self {
        let epoch = slot_swap_epoch_handle(slot);
        let seen = epoch.load(Ordering::SeqCst);
        SlotWatch {
            slot: Arc::clone(slot),
            epoch,
            seen: std::cell::Cell::new(seen),
        }
    }

    pub(crate) fn slot(&self) -> &Mutex<Connection> {
        &self.slot
    }

    pub(crate) fn observe_swap(&self) -> bool {
        let current = self.epoch.load(Ordering::SeqCst);
        if current == self.seen.get() {
            return false;
        }
        self.seen.set(current);
        true
    }
}

pub struct DbState {
    pub conn: Arc<Mutex<Connection>>,
    pub read_conn: Arc<Mutex<Connection>>,
}

impl DbState {
    pub fn from_pair(conn: Connection, read_conn: Connection) -> Self {
        DbState {
            conn: Arc::new(Mutex::new(conn)),
            read_conn: Arc::new(Mutex::new(read_conn)),
        }
    }

    pub fn open_in_memory() -> Result<DbState> {
        let mut conn = open_in_memory()?;
        init_db(&mut conn)?;
        let conn = Arc::new(Mutex::new(conn));
        Ok(DbState {
            read_conn: conn.clone(),
            conn,
        })
    }

    pub fn swap_pair(&self, conn: Connection, read_conn: Connection) -> Result<()> {
        let mut guard = self.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        *guard = conn;
        bump_slot_swap_epoch(&self.conn);
        drop(guard);
        self.replace_read_conn(read_conn)
    }

    pub(crate) fn replace_read_conn(&self, conn: Connection) -> Result<()> {
        replace_read_conn_slot(&self.read_conn, conn)
    }

    pub fn placeholderize_read_conn(&self) -> Result<()> {
        self.replace_read_conn(open_in_memory()?)
    }

    pub fn slots(&self) -> super::facade_handles::DbSlotPair {
        super::facade_handles::DbSlotPair::new(Arc::clone(&self.conn), Arc::clone(&self.read_conn))
    }

    pub fn write_handle(&self) -> super::facade_handles::DbWriteHandle {
        self.slots().write_handle()
    }

    pub fn read_handle(&self) -> super::facade_handles::DbReadHandle {
        self.slots().read_handle()
    }
}

pub fn replace_read_conn_slot(slot: &Arc<Mutex<Connection>>, conn: Connection) -> Result<()> {
    let mut guard = slot.lock().map_err(|e| AppError::Db(e.to_string()))?;
    drop(std::mem::replace(&mut *guard, conn));
    bump_slot_swap_epoch(slot);
    Ok(())
}
