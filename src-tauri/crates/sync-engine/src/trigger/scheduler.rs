use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tauri::{AppHandle, Manager, Runtime};

use crate::channel::{
    ChannelOptions, ConnSegment, RoundConn, SyncRoundReport, connection_round_key,
};
use crate::envelope::EnvelopeMode;
use ledger_infra::db::LOCK_HOLD_PROBE_THRESHOLD;
use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::{DbState, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_infra::settings::{self, SettingKey};
use ledger_infra::signals::{WriteEvidence, WriteOp, emit_for};

use super::channel::{SyncChannel, build_channel, configured_channel};
use super::round_gate::{RoundStart, begin_round};
use super::session::SessionEnvelope;

const POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);

const WRITE_DEBOUNCE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerTimings {
    pub poll_interval: Duration,
    pub write_debounce: Duration,
}

impl Default for TriggerTimings {
    fn default() -> Self {
        Self {
            poll_interval: POLL_INTERVAL,
            write_debounce: WRITE_DEBOUNCE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InFlightPolicy {
    WaitAndReuse,
    Skip,
}

enum GatedRound {
    Ran(SyncRoundReport),
    Joined(SyncRoundReport),
    Skipped,
}

pub fn run_round_once<S: RoundConn>(
    locks: &S,
    channel: &SyncChannel,
    mode: &EnvelopeMode<'_>,
) -> Result<SyncRoundReport> {
    match run_round_gated(locks, channel, mode, InFlightPolicy::WaitAndReuse) {
        Ok(GatedRound::Ran(report)) | Ok(GatedRound::Joined(report)) => Ok(report),
        Ok(GatedRound::Skipped) => Err(AppError::Invalid(
            "在途等待未获轮次报告（程序缺陷）".to_string(),
        )),
        Err(error) => Err(error),
    }
}

pub fn run_auto_round<S: RoundConn>(
    locks: &S,
    session: &SessionEnvelope,
) -> Result<Option<SyncRoundReport>> {
    match run_auto_round_inner(locks, session) {
        Err(error) if is_round_lock_give_up(&error) => Ok(None),
        other => other,
    }
}

fn run_auto_round_inner<S: RoundConn>(
    locks: &S,
    session: &SessionEnvelope,
) -> Result<Option<SyncRoundReport>> {
    let Some(config) = locks.with_connection(ConnSegment::Read, configured_channel)? else {
        return Ok(None);
    };
    let channel = build_channel(&config)?;
    Ok(
        match run_round_gated(locks, &channel, &session.mode(), InFlightPolicy::Skip)? {
            GatedRound::Ran(report) | GatedRound::Joined(report) => Some(report),
            GatedRound::Skipped => None,
        },
    )
}

fn run_round_gated<S: RoundConn>(
    locks: &S,
    channel: &SyncChannel,
    mode: &EnvelopeMode<'_>,
    policy: InFlightPolicy,
) -> Result<GatedRound> {
    match begin_round(locks.round_key()) {
        RoundStart::Began(handle) => {
            let outcome = run_round_and_bookkeep(locks, channel, mode);
            handle.complete(outcome.clone());
            match outcome {
                Ok(report) => Ok(GatedRound::Ran(report)),
                Err(error) => Err(error),
            }
        }
        RoundStart::InFlight(slot) => match policy {
            InFlightPolicy::WaitAndReuse => Ok(GatedRound::Joined(slot.wait()?)),
            InFlightPolicy::Skip => Ok(GatedRound::Skipped),
        },
    }
}

fn run_round_and_bookkeep<S: RoundConn>(
    locks: &S,
    channel: &SyncChannel,
    mode: &EnvelopeMode<'_>,
) -> Result<SyncRoundReport> {
    let report = channel.run_round(locks, mode, &ChannelOptions::default())?;
    locks.with_connection(ConnSegment::Bookkeep, |conn| {
        settings::set(conn, SettingKey::SyncLastSyncAt, &now_iso())
    })?;
    Ok(report)
}

fn round_lock_give_up_error() -> AppError {
    AppError::coded(
        ROUND_LOCK_GIVE_UP,
        "连接锁在时限内不可得，放弃本轮同步（下一轮重试）",
    )
}

const ROUND_LOCK_GIVE_UP: &str = "sync-engine.round-lock-give-up";

fn is_round_lock_give_up(error: &AppError) -> bool {
    error.is_code(ROUND_LOCK_GIVE_UP)
}

pub(crate) struct AutoRoundConn {
    conn: Arc<Mutex<Connection>>,
    lock_timeout: Duration,
    probe_threshold: Duration,
}

impl AutoRoundConn {
    pub(crate) fn new(conn: &Arc<Mutex<Connection>>) -> Self {
        Self {
            conn: Arc::clone(conn),
            lock_timeout: ledger_backup::LOCK_TIMEOUT,
            probe_threshold: LOCK_HOLD_PROBE_THRESHOLD,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_probe_threshold(mut self, threshold: Duration) -> Self {
        self.probe_threshold = threshold;
        self
    }
}

impl RoundConn for AutoRoundConn {
    fn with_connection<R, F>(&self, _segment: ConnSegment, use_connection: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        let hold_started = Instant::now();
        let Some(guard) = ledger_backup::lock_conn_within(&self.conn, self.lock_timeout) else {
            return Err(round_lock_give_up_error());
        };
        let result = use_connection(&guard);
        ledger_infra::db::probe_lock_hold_within(hold_started.elapsed(), self.probe_threshold);
        result
    }

    fn round_key(&self) -> u64 {
        connection_round_key(&self.conn)
    }
}

pub fn sync_on_start<R: Runtime>(app: &AppHandle<R>) {
    let conn = Arc::clone(&app.state::<DbState>().conn);
    let handle = app.clone();
    std::thread::spawn(move || {
        run_auto_round_with_emit(&handle, &conn, &SessionEnvelope::current());
    });
}

pub fn start_triggers<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(desktop)]
    start_sync_scheduler(app);
    sync_on_start(app);
}

pub(crate) fn drain_write_signals(rx: &std::sync::mpsc::Receiver<()>, window: Duration) {
    loop {
        match rx.recv_timeout(window) {
            Ok(()) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

static WRITE_SIGNAL: std::sync::OnceLock<std::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

pub fn install_after_write_hook() {
    let installed = ledger_sync_protocol::op::install_after_write_hook(sync_after_write);
    if !installed {
        tracing::debug!("写后钩子已安装，忽略重复登记（先装者优先）");
    }
}

pub fn sync_after_write() {
    notify_write_signal(&WRITE_SIGNAL);
}

pub(crate) fn notify_write_signal(slot: &std::sync::OnceLock<std::sync::mpsc::Sender<()>>) {
    if let Some(tx) = slot.get() {
        let _ = tx.send(());
    }
}

pub fn start_sync_scheduler<R: Runtime>(app: &AppHandle<R>) {
    start_sync_scheduler_with(app, TriggerTimings::default());
}

pub fn start_sync_scheduler_with<R: Runtime>(app: &AppHandle<R>, timings: TriggerTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    if WRITE_SIGNAL.set(tx).is_err() {
        tracing::warn!("同步调度已存在写信号通道，写后触发退化为低频轮询");
    }
    let conn = Arc::clone(&app.state::<DbState>().conn);
    let gate = EncryptionGate::clone(&app.state::<EncryptionGate>());
    let boot_gate = BootFailureGate::clone(&app.state::<BootFailureGate>());
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            match rx.recv_timeout(timings.poll_interval) {
                Ok(()) => {
                    drain_write_signals(&rx, timings.write_debounce);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if gate.is_locked() || boot_gate.is_failed() {
                continue;
            }
            run_auto_round_with_emit(&handle, &conn, &SessionEnvelope::current());
        }
    });
}

fn run_auto_round_with_emit<R: Runtime>(
    app: &AppHandle<R>,
    conn: &Arc<Mutex<Connection>>,
    session: &SessionEnvelope,
) {
    let locks = AutoRoundConn::new(conn);
    match run_auto_round(&locks, session) {
        Ok(Some(report)) => {
            tracing::debug!(
                applied = report.applied,
                parked = report.parked,
                "同步轮次完成"
            );
            emit_for(
                app,
                WriteOp::SyncRound,
                WriteEvidence::LedgerApplied(report.applied > 0),
            );
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "同步轮次失败（静默，等下一轮）"),
    }
}

pub fn not_configured_error() -> AppError {
    AppError::coded(
        "sync-channel.not-configured",
        "同步通道尚未配置，请先在设置中填写同步通道信息",
    )
}

pub fn book_unavailable_error() -> AppError {
    AppError::coded(
        "sync-channel.book-unavailable",
        "账本注册表不可用，无法确定同步范围",
    )
}
