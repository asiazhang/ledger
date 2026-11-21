use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Offset, TimeZone, Utc};
use rusqlite::Connection;

use ledger_infra::db::LockOutcome;
use ledger_infra::db::{self, DbState};
use ledger_infra::error::{self, AppError};
use ledger_infra::settings::{self, SettingKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupDecision {
    BackupNow,
    Clean,
    AlreadyBackedUpToday,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoBackupState {
    pub enabled: bool,
    pub dirty: bool,
    pub last_backup_at: Option<String>,
    pub consecutive_failures: u32,
}

impl Default for AutoBackupState {
    fn default() -> Self {
        Self {
            enabled: true,
            dirty: false,
            last_backup_at: None,
            consecutive_failures: 0,
        }
    }
}

pub fn due_decision<Tz: TimeZone>(
    dirty: bool,
    last_backup_at: Option<DateTime<Utc>>,
    now_local: DateTime<Tz>,
) -> BackupDecision {
    if !dirty {
        return BackupDecision::Clean;
    }
    if backed_up_today(last_backup_at, now_local) {
        BackupDecision::AlreadyBackedUpToday
    } else {
        BackupDecision::BackupNow
    }
}

fn backed_up_today<Tz: TimeZone>(
    last_backup_at: Option<DateTime<Utc>>,
    now_local: DateTime<Tz>,
) -> bool {
    match last_backup_at {
        None => false,
        Some(last) => {
            let offset = now_local.offset().fix();
            last.with_timezone(&offset).date_naive() >= now_local.date_naive()
        }
    }
}

fn anchor_of(state: &AutoBackupState) -> Option<DateTime<Utc>> {
    state.last_backup_at.as_deref().and_then(parse_iso)
}

fn to_local(now: DateTime<Utc>) -> DateTime<Local> {
    now.with_timezone(&Local)
}

pub const FAILURE_ALERT_THRESHOLD: u32 = 3;

pub fn failure_alerting(consecutive_failures: u32) -> bool {
    consecutive_failures >= FAILURE_ALERT_THRESHOLD
}

pub fn get_state(conn: &Connection) -> error::Result<AutoBackupState> {
    let def = AutoBackupState::default();
    Ok(AutoBackupState {
        enabled: settings::get(conn, SettingKey::AutoBackupEnabled, def.enabled)?,
        dirty: settings::get(conn, SettingKey::AutoBackupDirty, def.dirty)?,
        last_backup_at: settings::get(
            conn,
            SettingKey::AutoBackupLastBackupAt,
            def.last_backup_at,
        )?,
        consecutive_failures: settings::get(
            conn,
            SettingKey::AutoBackupConsecutiveFailures,
            def.consecutive_failures,
        )?,
    })
}

pub fn set_state(conn: &Connection, state: &AutoBackupState) -> error::Result<()> {
    settings::set(conn, SettingKey::AutoBackupEnabled, &state.enabled)?;
    settings::set(conn, SettingKey::AutoBackupDirty, &state.dirty)?;
    settings::set(
        conn,
        SettingKey::AutoBackupLastBackupAt,
        &state.last_backup_at,
    )?;
    settings::set(
        conn,
        SettingKey::AutoBackupConsecutiveFailures,
        &state.consecutive_failures,
    )?;
    Ok(())
}

fn mark_dirty(conn: &Connection) -> error::Result<()> {
    settings::set(conn, SettingKey::AutoBackupDirty, &true)
}

fn mark_dirty_ignoring_failure(conn: &Connection) {
    if let Err(e) = mark_dirty(conn) {
        tracing::warn!(error = %e, "写库成功但置脏失败（忽略）");
    }
}

pub fn after_commit_hook(conn: &Connection) {
    mark_dirty_ignoring_failure(conn);
    let dir = shared_prefs().snapshot_dir();
    run_due_backup(conn, dir.as_deref(), env!("CARGO_PKG_VERSION"), Utc::now());
}

pub fn install_after_commit_hook() {
    db::register_after_commit_hook(after_commit_hook);
}

pub fn occurrence_dirty_hook(conn: &Connection) {
    mark_dirty_ignoring_failure(conn);
}

type CatchUpHook = fn(&Connection);

static CATCH_UP_HOOK: OnceLock<CatchUpHook> = OnceLock::new();

pub fn register_catch_up_hook(hook: CatchUpHook) {
    let _ = CATCH_UP_HOOK.set(hook);
}

fn run_catch_up_hook(conn: &Connection) {
    match CATCH_UP_HOOK.get() {
        Some(hook) => hook(conn),
        None => tracing::error!("追补触发钩子未注册：定时计划追补本轮被跳过（壳层启动接线缺失）"),
    }
}

pub fn mark_clean(conn: &Connection, now: &str) -> error::Result<()> {
    settings::set(conn, SettingKey::AutoBackupDirty, &false)?;
    settings::set(conn, SettingKey::AutoBackupConsecutiveFailures, &0_u32)?;
    settings::set(
        conn,
        SettingKey::AutoBackupLastBackupAt,
        &Some(now.to_string()),
    )
}

pub fn reset(conn: &Connection, now: &str) -> error::Result<()> {
    mark_clean(conn, now)
}

pub const AUTO_BACKUP_PREFIX: &str = "ledger-auto-";

const CHECK_INTERVAL_SECS: u64 = 10 * 60;
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

pub fn auto_backup_file_name<Tz: TimeZone>(now: DateTime<Tz>, book: Option<&str>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    super::engine::managed_backup_file_name(
        AUTO_BACKUP_PREFIX,
        &now.format("%Y%m%d-%H%M%S").to_string(),
        book,
    )
}

fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    Performed { path: String },
    Skipped(SkipReason),
    Failed { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Disabled,
    DirMissing,
    NotDue,
    AlreadyBackedUpToday,
    Clean,
    ListNotEmpty,
}

fn perform_backup(
    conn: &Connection,
    dir: &str,
    app_version: &str,
    now: DateTime<Utc>,
) -> error::Result<String> {
    let scope = super::engine::BackupScope::of_conn(conn)?;
    let file_name = auto_backup_file_name(
        now.with_timezone(&Local),
        scope.as_ref().map(super::engine::BackupScope::book_tag),
    );
    let target = super::engine::managed_write_target(Path::new(dir), &file_name, conn)?;
    let path =
        super::engine::backup_db_to(conn, &target, app_version, super::engine::BackupKind::Auto)?
            .path;
    mark_clean(conn, &db::iso_at(now))?;
    Ok(path)
}

fn effective_dir(dir: Option<&str>) -> Option<&str> {
    dir.map(str::trim).filter(|d| !d.is_empty())
}

fn failed(reason: AppError) -> AttemptOutcome {
    AttemptOutcome::Failed {
        reason: reason.to_string(),
    }
}

fn gate(conn: &Connection, dir: Option<&str>) -> Result<(AutoBackupState, String), AttemptOutcome> {
    let state = get_state(conn).map_err(failed)?;
    if !state.enabled {
        return Err(AttemptOutcome::Skipped(SkipReason::Disabled));
    }
    let Some(dir) = effective_dir(dir) else {
        return Err(AttemptOutcome::Skipped(SkipReason::DirMissing));
    };
    Ok((state, dir.to_string()))
}

fn record_failure(conn: &Connection) {
    let prev: u32 = match settings::get(conn, SettingKey::AutoBackupConsecutiveFailures, 0) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "读取连续失败计数失败，本次不累加");
            return;
        }
    };
    let next = prev.saturating_add(1);
    if let Err(e) = settings::set(conn, SettingKey::AutoBackupConsecutiveFailures, &next) {
        tracing::warn!(error = %e, "写入连续失败计数失败（忽略）");
        return;
    }
    if prev < FAILURE_ALERT_THRESHOLD && failure_alerting(next) {
        tracing::warn!(count = next, "自动备份连续失败达提示阈值，通知前端呈现");
        notify_threshold_crossing();
    }
}

fn notify_threshold_crossing() {
    ledger_infra::events::emit_backups_changed_current();
    #[cfg(test)]
    THRESHOLD_ALERT_EMISSIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
static THRESHOLD_ALERT_EMISSIONS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

fn classify_result(
    conn: &Connection,
    trigger: &str,
    performed: error::Result<String>,
) -> AttemptOutcome {
    match performed {
        Ok(path) => {
            tracing::info!(trigger, path = %path, "自动备份完成");
            ledger_infra::events::emit_backups_changed_current();
            AttemptOutcome::Performed { path }
        }
        Err(e) => {
            tracing::warn!(trigger, error = %e, "自动备份失败，保留脏标记待下周期重试");
            record_failure(conn);
            AttemptOutcome::Failed {
                reason: e.to_string(),
            }
        }
    }
}

pub fn run_due_backup(
    conn: &Connection,
    dir: Option<&str>,
    app_version: &str,
    now: DateTime<Utc>,
) -> AttemptOutcome {
    let (state, dir) = match gate(conn, dir) {
        Ok(v) => v,
        Err(outcome) => return outcome,
    };
    let last = anchor_of(&state);
    match due_decision(state.dirty, last, to_local(now)) {
        BackupDecision::BackupNow => {}
        BackupDecision::Clean => return AttemptOutcome::Skipped(SkipReason::NotDue),
        BackupDecision::AlreadyBackedUpToday => {
            tracing::debug!(
                trigger = "due",
                "今天已自动备份，日界门静默跳过（先到先触发）"
            );
            return AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday);
        }
    }
    classify_result(conn, "due", perform_backup(conn, &dir, app_version, now))
}

pub fn run_exit_backup(
    conn: &Connection,
    dir: Option<&str>,
    app_version: &str,
    now: DateTime<Utc>,
) -> AttemptOutcome {
    let (state, dir) = match gate(conn, dir) {
        Ok(v) => v,
        Err(outcome) => return outcome,
    };
    let last = anchor_of(&state);
    match due_decision(state.dirty, last, to_local(now)) {
        BackupDecision::BackupNow => {}
        BackupDecision::Clean => return AttemptOutcome::Skipped(SkipReason::Clean),
        BackupDecision::AlreadyBackedUpToday => {
            tracing::debug!(trigger = "exit", "今天已自动备份，日界门静默跳过");
            return AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday);
        }
    }
    classify_result(conn, "exit", perform_backup(conn, &dir, app_version, now))
}

pub fn run_first_backup(
    conn: &Connection,
    dir: Option<&str>,
    app_version: &str,
    now: DateTime<Utc>,
) -> AttemptOutcome {
    let (state, dir) = match gate(conn, dir) {
        Ok(v) => v,
        Err(outcome) => return outcome,
    };
    let scoped_list = match super::engine::BackupScope::of_conn(conn) {
        Ok(Some(scope)) => super::engine::list_managed_backups(Path::new(&dir), Some(&scope)),
        Ok(None) => Ok(Vec::new()),
        Err(e) => Err(e),
    };
    match scoped_list {
        Ok(list) if !list.is_empty() => {
            return AttemptOutcome::Skipped(SkipReason::ListNotEmpty);
        }
        Ok(_) => {}
        Err(e) => return failed(e),
    }
    let last = anchor_of(&state);
    if backed_up_today(last, to_local(now)) {
        tracing::debug!(trigger = "first", "今天已自动备份，日界门静默跳过首次兜底");
        return AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday);
    }
    classify_result(conn, "first", perform_backup(conn, &dir, app_version, now))
}

static SHARED_PREFS: OnceLock<Arc<PrefsState>> = OnceLock::new();

pub fn shared_prefs() -> Arc<PrefsState> {
    SHARED_PREFS
        .get_or_init(|| Arc::new(PrefsState::default()))
        .clone()
}

#[derive(Default)]
pub struct PrefsState {
    pub dir: Arc<Mutex<Option<String>>>,
    first_fallback_claimed: AtomicBool,
}

impl PrefsState {
    fn lock_dir(&self) -> MutexGuard<'_, Option<String>> {
        self.dir.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_dir(&self, dir: Option<String>) {
        *self.lock_dir() = dir;
    }

    pub fn snapshot_dir(&self) -> Option<String> {
        self.lock_dir().clone()
    }

    pub fn claim_first_fallback(&self) -> bool {
        !self.first_fallback_claimed.swap(true, Ordering::SeqCst)
    }
}

pub fn lock_conn_with_timeout(conn: &Arc<Mutex<Connection>>) -> Option<MutexGuard<'_, Connection>> {
    lock_conn_within(conn, LOCK_TIMEOUT)
}

pub fn lock_conn_within(
    conn: &Arc<Mutex<Connection>>,
    timeout: Duration,
) -> Option<MutexGuard<'_, Connection>> {
    let deadline = Instant::now() + timeout;
    loop {
        match conn.try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::Poisoned(_)) => {
                tracing::warn!("数据库连接锁损坏，跳过本轮自动备份");
                return None;
            }
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    tracing::warn!(
                        timeout_ms = timeout.as_millis() as u64,
                        "等待数据库锁超时，跳过本轮自动备份"
                    );
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

static SCHEDULER_SPAWNED: AtomicBool = AtomicBool::new(false);

fn run_scheduled_round(conn: &Connection, dir: Option<&str>, version: &str) {
    run_due_backup(conn, dir, version, Utc::now());
    run_catch_up_hook(conn);
}

fn log_timed_outcome(trigger: &str, outcome: LockOutcome<()>) {
    match outcome {
        LockOutcome::Abandoned => {
            tracing::warn!(
                trigger,
                timeout_ms = LOCK_TIMEOUT.as_millis() as u64,
                "等待数据库连接超时，跳过本轮"
            );
        }
        LockOutcome::Ran(Ok(())) => {}
        LockOutcome::Ran(Err(error)) => {
            tracing::warn!(trigger, error = %error, "本轮备份作业失败，跳过本轮");
        }
    }
}

pub fn start_scheduler<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if SCHEDULER_SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let conn = app.state::<DbState>().write_handle();
    let gate = ledger_infra::db::encryption::EncryptionGate::clone(
        &app.state::<ledger_infra::db::encryption::EncryptionGate>(),
    );
    let boot_gate = ledger_infra::db::boot::BootFailureGate::clone(
        &app.state::<ledger_infra::db::boot::BootFailureGate>(),
    );
    let dir_mirror = Arc::clone(&shared_prefs().dir);
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(CHECK_INTERVAL_SECS));
            if gate.is_locked() || boot_gate.is_failed() {
                continue;
            }
            let version = handle.package_info().version.to_string();
            let outcome = conn.run_raw_blocking_within("backup.scheduler-round", LOCK_TIMEOUT, {
                let dir_mirror = Arc::clone(&dir_mirror);
                move |conn| {
                    let dir = dir_mirror.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    run_scheduled_round(conn, dir.as_deref(), &version);
                    Ok(())
                }
            });
            log_timed_outcome("scheduler-round", outcome);
        }
    });
}

pub fn exit_fallback(app: &tauri::AppHandle) {
    use tauri::Manager;
    let conn = app.state::<DbState>().write_handle();
    let dir = shared_prefs().snapshot_dir();
    let version = app.package_info().version.to_string();
    let outcome = conn.run_raw_blocking_within("backup.exit-fallback", LOCK_TIMEOUT, move |conn| {
        run_exit_backup(conn, dir.as_deref(), &version, Utc::now());
        Ok(())
    });
    log_timed_outcome("exit-fallback", outcome);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> rusqlite::Connection {
        tauri_app_lib::test_support::open()
    }

    fn ts(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .expect("合法 ISO 时间")
            .with_timezone(&chrono::Utc)
    }

    mod decision_table {
        use super::*;
        use chrono::FixedOffset;

        fn local_at(s: &str) -> DateTime<FixedOffset> {
            DateTime::parse_from_rfc3339(s).expect("合法 ISO 时间")
        }

        #[test]
        fn first_backup_when_never_backed_up() {
            let now = local_at("2026-02-17T10:00:00+08:00");
            assert_eq!(due_decision(true, None, now), BackupDecision::BackupNow);
            assert_eq!(due_decision(false, None, now), BackupDecision::Clean);
        }

        #[test]
        fn clean_means_no_backup() {
            let now = local_at("2026-02-17T10:00:00+08:00");
            assert_eq!(
                due_decision(false, Some(ts("2026-02-15T00:00:00Z")), now),
                BackupDecision::Clean
            );
        }

        #[test]
        fn already_backed_up_today_skips() {
            let now = local_at("2026-02-17T23:45:00+08:00");
            let anchor = ts("2026-02-16T16:15:00Z");
            assert_eq!(
                due_decision(true, Some(anchor), now),
                BackupDecision::AlreadyBackedUpToday
            );
            assert_eq!(
                due_decision(true, Some(now.with_timezone(&Utc)), now),
                BackupDecision::AlreadyBackedUpToday
            );
        }

        #[test]
        fn backed_up_yesterday_is_due_again() {
            let now = local_at("2026-02-18T00:30:00+08:00");
            let anchor = ts("2026-02-17T15:30:00Z");
            assert_eq!(
                due_decision(true, Some(anchor), now),
                BackupDecision::BackupNow
            );
        }

        #[test]
        fn clock_rollback_is_safe_side_skip() {
            let now = local_at("2026-02-17T10:00:00+08:00");
            let anchor = ts("2026-02-18T02:00:00Z");
            assert_eq!(
                due_decision(true, Some(anchor), now),
                BackupDecision::AlreadyBackedUpToday
            );
        }
    }

    #[test]
    fn get_state_defaults_when_keys_missing() {
        let c = conn();
        let state = get_state(&c).expect("读默认状态");
        assert_eq!(state, AutoBackupState::default());
    }

    #[test]
    fn get_state_defaults_when_table_missing() {
        let c = tauri_app_lib::test_support::open();
        c.execute("DROP TABLE app_settings", [])
            .expect("删除 app_settings");
        let state = get_state(&c).expect("缺表取默认状态");
        assert_eq!(state, AutoBackupState::default());
    }

    #[test]
    fn set_then_get_roundtrip() {
        let c = conn();
        let want = AutoBackupState {
            enabled: false,
            dirty: true,
            last_backup_at: Some(String::from("2026-02-17T08:00:00Z")),
            consecutive_failures: 5,
        };
        set_state(&c, &want).expect("写状态");
        assert_eq!(get_state(&c).expect("读回状态"), want);
    }

    #[test]
    fn mark_dirty_persists() {
        let c = conn();
        mark_dirty(&c).expect("置脏");
        let state = get_state(&c).expect("读状态");
        assert!(state.dirty);
    }

    #[test]
    fn mark_clean_resets_dirty_and_anchors_time() {
        let c = conn();
        mark_dirty(&c).expect("置脏");
        mark_clean(&c, "2026-02-17T09:30:00Z").expect("置洁");
        let state = get_state(&c).expect("读状态");
        assert!(!state.dirty);
        assert_eq!(
            state.last_backup_at,
            Some(String::from("2026-02-17T09:30:00Z"))
        );
    }

    #[test]
    fn failure_alerting_threshold_boundary() {
        for n in [0_u32, 1, 2] {
            assert!(!failure_alerting(n), "{n} 次连续失败不应提示");
        }
        for n in [
            FAILURE_ALERT_THRESHOLD,
            FAILURE_ALERT_THRESHOLD + 1,
            u32::MAX,
        ] {
            assert!(failure_alerting(n), "{n} 次连续失败应提示");
        }
    }

    #[test]
    fn reset_clears_dirty_and_reanchors() {
        let c = conn();
        set_state(
            &c,
            &AutoBackupState {
                enabled: false,
                dirty: true,
                last_backup_at: Some(String::from("2026-02-10T00:00:00Z")),
                consecutive_failures: 2,
            },
        )
        .expect("写脏状态");
        reset(&c, "2026-02-17T10:00:00Z").expect("重置");
        let state = get_state(&c).expect("读状态");
        assert!(!state.dirty);
        assert!(!state.enabled);
        assert_eq!(
            state.consecutive_failures, 0,
            "恢复后失败计数清零（全新起点）"
        );
        assert_eq!(
            state.last_backup_at,
            Some(String::from("2026-02-17T10:00:00Z"))
        );
    }
}

#[cfg(test)]
mod scheduler_tests {
    use super::*;
    use std::fs;
    use tauri_app_lib::test_support::ScratchDir;

    fn conn() -> rusqlite::Connection {
        tauri_app_lib::test_support::open()
    }

    fn temp_dir(tag: &str) -> ScratchDir {
        ScratchDir::new(&format!("auto-test-{tag}"))
    }

    static CROSSING_TESTS: Mutex<()> = Mutex::new(());

    fn now_at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .expect("合法 ISO 时间")
            .with_timezone(&Utc)
    }

    fn local_utc(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> DateTime<Utc> {
        use chrono::TimeZone;
        Local
            .with_ymd_and_hms(y, m, d, hh, mm, 0)
            .single()
            .expect("无歧义本地时刻")
            .with_timezone(&Utc)
    }

    fn local_noon_days_ago_utc(days_ago: u64) -> DateTime<Utc> {
        use chrono::TimeZone;
        let date = Local::now().date_naive() - chrono::Days::new(days_ago);
        Local
            .from_local_datetime(&date.and_hms_opt(12, 0, 0).unwrap())
            .earliest()
            .expect("本地正午可解析")
            .with_timezone(&Utc)
    }

    #[test]
    fn file_name_uses_local_time_of_injected_instant() {
        let tz = chrono::FixedOffset::east_opt(8 * 3600).expect("固定偏移");
        assert_eq!(
            auto_backup_file_name(now_at("2026-02-17T09:30:00Z").with_timezone(&tz), None),
            "ledger-auto-20260217-173000.db.zip"
        );
        assert_eq!(
            auto_backup_file_name(now_at("2026-02-17T16:30:00Z").with_timezone(&tz), None),
            "ledger-auto-20260218-003000.db.zip"
        );
        assert_eq!(
            auto_backup_file_name(
                now_at("2026-02-17T09:30:00Z").with_timezone(&tz),
                Some("book-a")
            ),
            "ledger-auto-20260217-173000-book-a.db.zip"
        );
    }

    #[test]
    fn due_backup_performs_and_marks_clean() {
        let c = conn();
        let dir = temp_dir("due-perform");
        mark_dirty(&c).expect("置脏");
        let outcome = run_due_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T12:00:00Z"),
        );
        match &outcome {
            AttemptOutcome::Performed { path } => {
                assert!(Path::new(path).is_file(), "产物文件应存在");
                assert_eq!(
                    Path::new(path).file_name().unwrap().to_str().unwrap(),
                    format!(
                        "ledger-auto-{}.db.zip",
                        now_at("2026-02-17T12:00:00Z")
                            .with_timezone(&Local)
                            .format("%Y%m%d-%H%M%S")
                    )
                );
                assert_eq!(
                    crate::read_backup_kind(Path::new(path)).unwrap(),
                    crate::BackupKind::Auto
                );
            }
            other => panic!("应执行备份，实际 {other:?}"),
        }
        let state = get_state(&c).expect("读状态");
        assert!(!state.dirty, "成功后应清真");
        assert_eq!(
            state.last_backup_at,
            Some(String::from("2026-02-17T12:00:00Z"))
        );
    }

    #[test]
    fn product_lands_in_device_subdir() {
        let c = conn();
        let dir = temp_dir("device-subdir");
        mark_dirty(&c).expect("置脏");
        let device = crate::identity::source_device_id(&c).expect("工厂库可取设备标识");
        let outcome = run_due_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T12:00:00Z"),
        );
        match &outcome {
            AttemptOutcome::Performed { path } => {
                assert_eq!(
                    Path::new(path).parent().unwrap(),
                    dir.join(&device),
                    "产物应落在设备子目录内"
                );
                assert!(Path::new(path).is_file(), "产物文件应存在");
            }
            other => panic!("应执行备份，实际 {other:?}"),
        }
    }

    #[test]
    fn due_backup_skips_when_already_backed_up_today() {
        let c = conn();
        let dir = temp_dir("due-same-day");
        set_state(
            &c,
            &AutoBackupState {
                enabled: true,
                dirty: true,
                last_backup_at: Some(db::iso_at(local_utc(2026, 2, 17, 8, 0))),
                consecutive_failures: 0,
            },
        )
        .expect("写状态");
        let outcome = run_due_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            local_utc(2026, 2, 17, 20, 0),
        );
        assert_eq!(
            outcome,
            AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday)
        );
        assert!(
            fs::read_dir(&dir).expect("列目录").next().is_none(),
            "不应产生文件"
        );
        let state = get_state(&c).expect("读状态");
        assert!(state.dirty, "跳过不清脏，次日补上");
        assert_eq!(
            state.last_backup_at,
            Some(db::iso_at(local_utc(2026, 2, 17, 8, 0))),
            "跳过不前移锚点"
        );
    }

    #[test]
    fn due_backup_recovers_next_local_day() {
        let c = conn();
        let dir = temp_dir("due-next-day");
        set_state(
            &c,
            &AutoBackupState {
                enabled: true,
                dirty: true,
                last_backup_at: Some(db::iso_at(local_noon_days_ago_utc(1))),
                consecutive_failures: 0,
            },
        )
        .expect("写状态");
        let now = local_noon_days_ago_utc(0);
        let outcome = run_due_backup(&c, Some(dir.to_str().unwrap()), "0.2.0", now);
        assert!(
            matches!(outcome, AttemptOutcome::Performed { .. }),
            "跨本地日后有变动应恢复备份，实际 {outcome:?}"
        );
        let state = get_state(&c).expect("读状态");
        assert_eq!(
            state.last_backup_at,
            Some(db::iso_at(now)),
            "成功后锚点前移到本次备份时刻"
        );
    }

    #[test]
    fn failure_retry_same_day_succeeds() {
        let c = conn();
        let dir = temp_dir("failure-retry");
        mark_dirty(&c).expect("置脏");
        let missing_root = ScratchDir::new("auto-missing");
        let missing = missing_root.join("nested");
        let first = run_due_backup(&c, Some(missing.to_str().unwrap()), "0.2.0", Utc::now());
        assert!(
            matches!(first, AttemptOutcome::Failed { .. }),
            "实际 {first:?}"
        );
        assert!(get_state(&c).unwrap().dirty, "失败必须保留脏标记");
        let second = run_due_backup(&c, Some(dir.to_str().unwrap()), "0.2.0", Utc::now());
        assert!(
            matches!(second, AttemptOutcome::Performed { .. }),
            "同日重试应可行，实际 {second:?}"
        );
        let state = get_state(&c).expect("读状态");
        assert!(
            !state.dirty && state.last_backup_at.is_some(),
            "重试成功后清真并记锚点"
        );
    }

    #[test]
    fn due_backup_silent_skip_without_dir() {
        let c = conn();
        mark_dirty(&c).expect("置脏");
        for dir in [None, Some(""), Some("   ")] {
            let outcome = run_due_backup(&c, dir, "0.2.0", now_at("2026-02-17T12:00:00Z"));
            assert_eq!(outcome, AttemptOutcome::Skipped(SkipReason::DirMissing));
        }
        let state = get_state(&c).unwrap();
        assert!(
            state.dirty && state.last_backup_at.is_none(),
            "静默跳过不改状态"
        );
    }

    #[test]
    fn disabled_means_skip_everywhere() {
        let c = conn();
        let dir = temp_dir("disabled");
        mark_dirty(&c).expect("置脏");
        settings::set(&c, SettingKey::AutoBackupEnabled, &false).expect("关开关");
        assert_eq!(
            run_due_backup(
                &c,
                Some(dir.to_str().unwrap()),
                "0.2.0",
                now_at("2026-02-17T12:00:00Z")
            ),
            AttemptOutcome::Skipped(SkipReason::Disabled)
        );
        assert_eq!(
            run_exit_backup(
                &c,
                Some(dir.to_str().unwrap()),
                "0.2.0",
                now_at("2026-02-17T12:00:00Z")
            ),
            AttemptOutcome::Skipped(SkipReason::Disabled)
        );
        assert_eq!(
            run_first_backup(
                &c,
                Some(dir.to_str().unwrap()),
                "0.2.0",
                now_at("2026-02-17T12:00:00Z")
            ),
            AttemptOutcome::Skipped(SkipReason::Disabled)
        );
        assert!(fs::read_dir(&dir).expect("列目录").next().is_none());
    }

    #[test]
    fn failure_keeps_dirty() {
        let c = conn();
        let missing_root = ScratchDir::new("auto-missing");
        let missing = missing_root.join("nested");
        mark_dirty(&c).expect("置脏");
        let outcome = run_due_backup(
            &c,
            Some(missing.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T12:00:00Z"),
        );
        assert!(
            matches!(outcome, AttemptOutcome::Failed { .. }),
            "实际 {outcome:?}"
        );
        let state = get_state(&c).unwrap();
        assert!(state.dirty, "失败必须保留脏标记");
        assert_eq!(state.last_backup_at, None);
    }

    #[test]
    fn consecutive_failures_accumulate_to_threshold() {
        let _serial = CROSSING_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let c = conn();
        let missing_root = ScratchDir::new("auto-fail-count");
        let missing = missing_root.join("nested");
        mark_dirty(&c).expect("置脏");
        for n in 1..=FAILURE_ALERT_THRESHOLD {
            let outcome = run_due_backup(
                &c,
                Some(missing.to_str().unwrap()),
                "0.2.0",
                now_at("2026-02-17T12:00:00Z"),
            );
            assert!(
                matches!(outcome, AttemptOutcome::Failed { .. }),
                "第 {n} 次应失败，实际 {outcome:?}"
            );
            let state = get_state(&c).expect("读状态");
            assert_eq!(state.consecutive_failures, n, "失败一次累加一");
            assert_eq!(
                failure_alerting(state.consecutive_failures),
                n >= FAILURE_ALERT_THRESHOLD,
                "达阈值起为提示态"
            );
            assert!(state.dirty, "计数写入不得清脏或置脏");
            assert_eq!(state.last_backup_at, None, "计数写入不记锚点");
            assert!(state.enabled, "计数写入不动开关");
        }
    }

    #[test]
    fn success_resets_failure_counter() {
        let _serial = CROSSING_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let c = conn();
        let missing_root = ScratchDir::new("auto-fail-reset");
        let missing = missing_root.join("nested");
        mark_dirty(&c).expect("置脏");
        for _ in 0..FAILURE_ALERT_THRESHOLD {
            run_due_backup(
                &c,
                Some(missing.to_str().unwrap()),
                "0.2.0",
                now_at("2026-02-17T12:00:00Z"),
            );
        }
        assert!(
            failure_alerting(get_state(&c).expect("读状态").consecutive_failures),
            "前置：已达提示态"
        );
        let dir = temp_dir("failure-reset");
        let outcome = run_due_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T13:00:00Z"),
        );
        assert!(
            matches!(outcome, AttemptOutcome::Performed { .. }),
            "实际 {outcome:?}"
        );
        let state = get_state(&c).expect("读状态");
        assert_eq!(state.consecutive_failures, 0, "成功后计数清零");
        assert!(!failure_alerting(state.consecutive_failures), "提示态解除");
    }

    #[test]
    fn threshold_crossing_emits_notification_once() {
        let _serial = CROSSING_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let c = conn();
        let missing_root = ScratchDir::new("auto-cross-emit");
        let missing = missing_root.join("nested");
        mark_dirty(&c).expect("置脏");
        THRESHOLD_ALERT_EMISSIONS.store(0, std::sync::atomic::Ordering::Relaxed);
        for n in 1..=(FAILURE_ALERT_THRESHOLD + 1) {
            let outcome = run_due_backup(
                &c,
                Some(missing.to_str().unwrap()),
                "0.2.0",
                now_at("2026-02-17T12:00:00Z"),
            );
            assert!(
                matches!(outcome, AttemptOutcome::Failed { .. }),
                "第 {n} 次应失败，实际 {outcome:?}"
            );
            let expected = u32::from(n >= FAILURE_ALERT_THRESHOLD);
            assert_eq!(
                THRESHOLD_ALERT_EMISSIONS.load(std::sync::atomic::Ordering::Relaxed),
                expected,
                "第 {n} 次失败后应累计通知 {expected} 次（恰跨越时一次，不重复发）"
            );
        }
    }

    #[test]
    fn exit_backup_skips_when_already_backed_up_today() {
        let c = conn();
        let dir = temp_dir("exit-same-day");
        set_state(
            &c,
            &AutoBackupState {
                enabled: true,
                dirty: true,
                last_backup_at: Some(db::iso_at(local_utc(2026, 2, 17, 8, 0))),
                consecutive_failures: 0,
            },
        )
        .expect("写状态");
        let outcome = run_exit_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            local_utc(2026, 2, 17, 20, 0),
        );
        assert_eq!(
            outcome,
            AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday)
        );
        assert!(fs::read_dir(&dir).expect("列目录").next().is_none());
    }

    #[test]
    fn exit_backup_performs_when_today_not_backed_up() {
        let c = conn();
        let dir = temp_dir("exit-next-day");
        set_state(
            &c,
            &AutoBackupState {
                enabled: true,
                dirty: true,
                last_backup_at: Some(db::iso_at(local_noon_days_ago_utc(1))),
                consecutive_failures: 0,
            },
        )
        .expect("写状态");
        let outcome = run_exit_backup(&c, Some(dir.to_str().unwrap()), "0.2.0", Utc::now());
        assert!(matches!(outcome, AttemptOutcome::Performed { .. }));
    }

    #[test]
    fn exit_backup_skips_when_clean() {
        let c = conn();
        let dir = temp_dir("exit-clean");
        let outcome = run_exit_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T12:00:00Z"),
        );
        assert_eq!(outcome, AttemptOutcome::Skipped(SkipReason::Clean));
        assert!(fs::read_dir(&dir).expect("列目录").next().is_none());
    }

    #[test]
    fn first_fallback_only_once_while_list_empty() {
        let c = conn();
        let dir = temp_dir("first-fallback");
        let d = dir.to_str().unwrap();
        let first = run_first_backup(&c, Some(d), "0.2.0", now_at("2026-02-17T08:00:00Z"));
        assert!(
            matches!(first, AttemptOutcome::Performed { .. }),
            "首次应兜底，实际 {first:?}"
        );
        let again = run_first_backup(&c, Some(d), "0.2.0", now_at("2026-02-17T09:00:00Z"));
        assert_eq!(again, AttemptOutcome::Skipped(SkipReason::ListNotEmpty));
    }

    #[test]
    fn first_fallback_skips_when_already_backed_up_today() {
        let c = conn();
        let dir = temp_dir("first-same-day");
        set_state(
            &c,
            &AutoBackupState {
                enabled: true,
                dirty: false,
                last_backup_at: Some(db::iso_at(local_utc(2026, 2, 17, 8, 0))),
                consecutive_failures: 0,
            },
        )
        .expect("写状态");
        let outcome = run_first_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            local_utc(2026, 2, 17, 20, 0),
        );
        assert_eq!(
            outcome,
            AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday)
        );
        assert!(fs::read_dir(&dir).expect("列目录").next().is_none());
    }

    #[test]
    fn first_fallback_counts_manual_backups() {
        let c = conn();
        ledger_infra::settings::set(
            &c,
            ledger_infra::settings::SettingKey::BookFingerprint,
            &"fp-manual-stub".to_string(),
        )
        .expect("预置固化指纹");
        let dir = temp_dir("first-manual");
        fs::write(
            dir.join("ledger-backup-20260101-000000-fp-manual-stub.db.zip"),
            b"stub",
        )
        .expect("造本账手动备份占位");
        let outcome = run_first_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T08:00:00Z"),
        );
        assert_eq!(outcome, AttemptOutcome::Skipped(SkipReason::ListNotEmpty));
        assert!(
            fs::read_dir(&dir).expect("列目录").count() == 1,
            "不应新增文件"
        );
    }

    fn scheduled_state(c: rusqlite::Connection) -> ledger_infra::db::DbState {
        let conn = Arc::new(Mutex::new(c));
        ledger_infra::db::DbState {
            read_conn: Arc::clone(&conn),
            conn,
        }
    }

    #[test]
    fn timed_round_gives_up_while_connection_is_busy() {
        let c = conn();
        mark_dirty(&c).expect("置脏");
        let dir = temp_dir("timed-round-busy");
        let state = scheduled_state(c);
        let handle = state.write_handle();
        let hold = state.conn.lock().expect("测试应拿到写槽");
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let handle = &handle;
            let dir_for_job = dir.to_str().unwrap().to_string();
            scope.spawn(move || {
                let outcome = handle.run_raw_blocking_within(
                    "backup.scheduler-round",
                    Duration::from_millis(50),
                    move |conn| {
                        Ok(run_due_backup(
                            conn,
                            Some(dir_for_job.as_str()),
                            "0.2.0",
                            Utc::now(),
                        ))
                    },
                );
                let _ = done_tx.send(outcome);
            });
            let outcome = done_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("弃权应在时限附近返回：作业体若未被局限时放弃，这里拿不到结果");
            assert!(
                matches!(outcome, LockOutcome::Abandoned),
                "等连接超时本轮应放弃（零执行），实际 {outcome:?}"
            );
        });
        drop(hold);
        assert!(
            get_state(&state.conn.lock().expect("取回写槽"))
                .expect("读状态")
                .dirty,
            "放弃本轮不得复位脏标记（保留给下个周期重试）"
        );
        assert!(
            fs::read_dir(&dir).expect("列目录").next().is_none(),
            "放弃本轮不得产生备份产物"
        );
        let out =
            handle.run_raw_blocking_within("backup.scheduler-round", Duration::from_secs(5), {
                let dir = dir.to_str().unwrap().to_string();
                move |conn| {
                    Ok(run_due_backup(
                        conn,
                        Some(dir.as_str()),
                        "0.2.0",
                        Utc::now(),
                    ))
                }
            });
        match out {
            LockOutcome::Ran(Ok(AttemptOutcome::Performed { path })) => {
                assert!(std::path::Path::new(&path).exists(), "重试应落下产物");
            }
            other => panic!("连接空闲后的下一轮应正常执行，实际 {other:?}"),
        }
        assert!(
            !get_state(&state.conn.lock().expect("取回写槽"))
                .expect("读状态")
                .dirty,
            "成功备份应复位脏标记"
        );
    }
}

#[cfg(test)]
mod book_scope_tests {
    use super::*;
    use std::fs;
    use tauri_app_lib::test_support::ScratchDir;

    fn conn() -> rusqlite::Connection {
        tauri_app_lib::test_support::open()
    }

    fn now_at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .expect("合法 ISO 时间")
            .with_timezone(&Utc)
    }

    fn temp_dir(tag: &str) -> ScratchDir {
        ScratchDir::new(&format!("auto-scope-{tag}"))
    }

    #[test]
    fn first_fallback_is_scoped_per_book() {
        let c = conn();
        let dir = temp_dir("per-book");
        fs::write(
            dir.join("ledger-auto-20260101-000000-foreign.db.zip"),
            b"stub",
        )
        .expect("造不可归属的存量产物");
        let outcome = run_first_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T08:00:00Z"),
        );
        match &outcome {
            AttemptOutcome::Performed { path } => {
                let name = Path::new(path).file_name().unwrap().to_str().unwrap();
                let expected = format!(
                    "ledger-auto-{}.db.zip",
                    now_at("2026-02-17T08:00:00Z")
                        .with_timezone(&Local)
                        .format("%Y%m%d-%H%M%S")
                );
                assert_eq!(name, expected, "产物命名按本地时间渲染（空库退化无标识）");
            }
            other => panic!("本账应有自己的首次兜底，实际 {other:?}"),
        }
        let again = run_first_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T09:00:00Z"),
        );
        assert_eq!(again, AttemptOutcome::Skipped(SkipReason::ListNotEmpty));
    }
}
