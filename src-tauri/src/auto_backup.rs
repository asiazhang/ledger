//! 自动备份（AutoBackup）状态、到期判定与调度（issue #124 / #125）。
//!
//! 职责边界：
//! - 状态经 [`crate::settings`] 收口持久化到 `app_settings` KV 表（`auto_backup.*` key），
//!   不建专表；IPC 由 `commands::backup` 的领域命令形状暴露（issue #128 接前端）；
//! - 到期判定为纯函数 [`due_decision`]，当前时间由调用方注入，不依赖全局时钟——
//!   **线程只做周期调用，是否备份的决策全在纯函数**（可测边界）；
//! - 调度层（issue #125）提供三种触发入口（周期到期 / 退出兜底 / 首次兜底），
//!   全部收敛到同一执行函数 [`perform_backup`]；轮询线程为标准模式
//!   （spawn + sleep），锁等待带超时，超时跳过本轮；
//! - `backupDir` 保持前端 localStorage 单一来源（ADR-0016 决策 3），后端只维护
//!   一份运行时镜像 [`PrefsState`]（启动时经 IPC `set_auto_backup_dir` 推送），
//!   目录未配置时一律静默跳过；
//! - 业务写库成功后的统一后置入口 [`on_write`]（issue #126）：置脏 + 写时顺带检查
//!   （到期且脏则立即备份）。各写路径（Writer 接缝 / 参考 CRUD / 市场数据写入）
//!   均调用它，深度模块只持有 `&Connection`，不经 AppHandle 取偏好——目录镜像经
//!   进程级单例 [`shared_prefs`] 读取，应用版本用编译期常量。
//!
//! 备注：`SettingKey::AutoBackupNextDueAt` 已预留但本模块不读写——锚点模型下
//! 「下次到期时间」可由 `last_backup_at + 间隔` 推导，不落地派生数据。

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::Connection;

use crate::db::{self, DbState};
use crate::settings::{self, SettingKey};

/// 自动备份间隔：距上次备份 ≥24h 才视为到期（备份频率上限每天一次）。
pub const AUTO_BACKUP_INTERVAL: TimeDelta = TimeDelta::hours(24);

/// 到期判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupDecision {
    /// 到期，应立即执行自动备份。
    BackupNow,
    /// 未到期或无需备份，本轮跳过。
    Skip,
}

/// 自动备份调度状态快照（对应 `auto_backup.*` 三个 KV key）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoBackupState {
    /// 自动备份开关（默认开启）。
    pub enabled: bool,
    /// 脏标记：数据变动后置真，备份成功后复位。
    pub dirty: bool,
    /// 上次成功备份时间（UTC ISO）；None 表示从未备份过。
    pub last_backup_at: Option<String>,
}

impl Default for AutoBackupState {
    fn default() -> Self {
        Self {
            enabled: true,
            dirty: false,
            last_backup_at: None,
        }
    }
}

/// 到期判定纯函数。
///
/// 语义：脏 且（从未备份 或 距上次备份 ≥ `interval`）→ [`BackupDecision::BackupNow`]。
/// 当前时间 `now` 由调用方注入；未到期时即使脏也跳过，保证每天最多一次。
///
/// 边界约定：
/// - 首次兜底（备份列表为空即备份一次）不属于本判定——它与脏标记无关，
///   由后续调度 ticket 在启动时检查备份列表自行处理；
/// - `last_backup_at` 晚于 `now`（时钟回拨/换设备导入）时差值为负、恒未到期，
///   即视为「刚备份过」安全侧处理，随时间推移自然恢复。
pub fn due_decision(
    dirty: bool,
    last_backup_at: Option<DateTime<Utc>>,
    interval: TimeDelta,
    now: DateTime<Utc>,
) -> BackupDecision {
    let due = match last_backup_at {
        // 从未备份过：无锚点，只要脏即视为到期（下一次检查机会就备份）。
        None => true,
        Some(last) => now.signed_duration_since(last) >= interval,
    };
    if dirty && due {
        BackupDecision::BackupNow
    } else {
        BackupDecision::Skip
    }
}

/// 读取自动备份调度状态。key 缺失、甚至 `app_settings` 表缺失
/// （恢复了旧版本备份）时返回约定默认值，行为免费正确。
pub fn get_state(conn: &Connection) -> crate::error::Result<AutoBackupState> {
    let def = AutoBackupState::default();
    Ok(AutoBackupState {
        enabled: settings::get(conn, SettingKey::AutoBackupEnabled, def.enabled)?,
        dirty: settings::get(conn, SettingKey::AutoBackupDirty, def.dirty)?,
        last_backup_at: settings::get(
            conn,
            SettingKey::AutoBackupLastBackupAt,
            def.last_backup_at,
        )?,
    })
}

/// 整体写入调度状态（三个 key 原子性无要求，逐个 upsert 即可）。
pub fn set_state(conn: &Connection, state: &AutoBackupState) -> crate::error::Result<()> {
    settings::set(conn, SettingKey::AutoBackupEnabled, &state.enabled)?;
    settings::set(conn, SettingKey::AutoBackupDirty, &state.dirty)?;
    settings::set(
        conn,
        SettingKey::AutoBackupLastBackupAt,
        &state.last_backup_at,
    )?;
    Ok(())
}

/// 置脏：任何业务写库成功后由写路径调用（Writer 接缝 / 参考 CRUD 等，后续 ticket 接线）。
pub fn mark_dirty(conn: &Connection) -> crate::error::Result<()> {
    settings::set(conn, SettingKey::AutoBackupDirty, &true)
}

/// 脏复位并把备份成功时刻记为新的上次备份锚点：
/// - [`mark_clean`]：自动备份成功后调用；失败时不得调用——保留脏标记即重试机制；
/// - [`reset`]：恢复成功后调用——不置真、重新计时，避免「恢复后立即备份」的重复，
///   开关保持恢复库中带来的值不动。
///
/// 两者行为一致（同一语义动作的两个领域别名），`now` 为 UTC ISO 字符串。
pub fn mark_clean(conn: &Connection, now: &str) -> crate::error::Result<()> {
    settings::set(conn, SettingKey::AutoBackupDirty, &false)?;
    settings::set(
        conn,
        SettingKey::AutoBackupLastBackupAt,
        &Some(now.to_string()),
    )
}

/// 见 [`mark_clean`]：恢复成功后重置调度状态。
pub fn reset(conn: &Connection, now: &str) -> crate::error::Result<()> {
    mark_clean(conn, now)
}

// ---------------------------------------------------------------------------
// 调度执行（issue #125）：触发入口 → 决策（纯函数）→ 执行
// ---------------------------------------------------------------------------

/// 自动备份产物命名前缀：`ledger-auto-YYYYMMDD-HHMMSS.db.zip`。
/// 后端受管备份判定（`commands::backup::core`）与前端命名/判定共用该语义（T3）。
pub const AUTO_BACKUP_PREFIX: &str = "ledger-auto-";

/// 轮询检查周期：每 30 分钟醒来检查一次到期判定（ADR-0016）。
const CHECK_INTERVAL_SECS: u64 = 30 * 60;
/// 执行备份时等待 DB 连接锁的超时；超时跳过本轮、保留脏标记，下个周期重试。
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// 自动备份产物文件名：`ledger-auto-YYYYMMDD-HHMMSS.db.zip`。
pub fn auto_backup_file_name(now: DateTime<Utc>) -> String {
    format!("{AUTO_BACKUP_PREFIX}{}.db.zip", now.format("%Y%m%d-%H%M%S"))
}

/// 解析 `last_backup_at` 存储格式（UTC ISO）。存储侧只写本模块生成的合法值，
/// 解析失败（旧数据脏写等极端情况）按「从未备份」处理：无锚点，脏即到期。
fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// 一次备份尝试的结果。调用方据此打日志/做后续动作；测试据此断言行为。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// 已成功创建备份并清真（记录锚点）。
    Performed { path: String },
    /// 静默跳过：不执行、不报错（含原因）。
    Skipped(SkipReason),
    /// 尝试了但失败：脏标记保留，下个周期重试（保留即重试机制，ADR-0016）。
    Failed { reason: String },
}

/// 静默跳过的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// 自动备份开关关闭（用户显式退出保护网，三种入口统一尊重）。
    Disabled,
    /// 备份目录未配置：不执行、不报错，设置页负责引导（Story 7）。
    DirMissing,
    /// 不满足触发条件（未到期 / 无变化），按入口语义判定。
    NotDue,
    /// 退出兜底专属：数据无变化（不脏），无事可做。
    Clean,
    /// 首次兜底专属：备份列表非空（不分手动/自动），无需兜底。
    ListNotEmpty,
}

/// 唯一执行落点：生成自动命名产物 + 成功后 [`mark_clean`]。
/// 失败时不上抛由调用方归入 [`AttemptOutcome::Failed`]——脏标记自然保留。
fn perform_backup(
    conn: &Connection,
    dir: &str,
    app_version: &str,
    now: DateTime<Utc>,
) -> crate::error::Result<String> {
    let target = Path::new(dir).join(auto_backup_file_name(now));
    let path = crate::commands::backup::backup_db_to(
        conn,
        &target,
        app_version,
        crate::commands::backup::BackupKind::Auto,
    )?
    .path;
    mark_clean(conn, &db::iso_at(now))?;
    Ok(path)
}

/// 目录参数归一化：`None` / 空白串一律视为未配置（避免空路径把产物落到当前目录）。
fn effective_dir(dir: Option<&str>) -> Option<&str> {
    dir.map(str::trim).filter(|d| !d.is_empty())
}

fn failed(reason: crate::error::AppError) -> AttemptOutcome {
    AttemptOutcome::Failed {
        reason: reason.to_string(),
    }
}

/// 三种触发入口共享的前奏：读状态 + 统一门禁（开关、目录），
/// 入口内只保留各自差异化的触发判定（到期 / 脏 / 列表空）。
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

/// 把执行结果归一化为 [`AttemptOutcome`] 并打日志（失败 warn，成功 info）。成功
/// 产物改变备份列表，一并发出 `ledger:backups-changed` 信号（issue #129），
/// 前端设置页据此自动刷新列表；发射失败静默忽略。
fn classify_result(trigger: &str, performed: crate::error::Result<String>) -> AttemptOutcome {
    match performed {
        Ok(path) => {
            tracing::info!(trigger, path = %path, "自动备份完成");
            crate::events::emit_backups_changed_current();
            AttemptOutcome::Performed { path }
        }
        Err(e) => {
            tracing::warn!(trigger, error = %e, "自动备份失败，保留脏标记待下周期重试");
            AttemptOutcome::Failed {
                reason: e.to_string(),
            }
        }
    }
}

/// 触发入口一：周期到期尝试（轮询线程用；issue #126 的写时顺带检查也复用）。
/// 读状态 → [`due_decision`] 纯函数决策 → 命中则执行。
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
    let last = state.last_backup_at.as_deref().and_then(parse_iso);
    if due_decision(state.dirty, last, AUTO_BACKUP_INTERVAL, now) != BackupDecision::BackupNow {
        return AttemptOutcome::Skipped(SkipReason::NotDue);
    }
    classify_result("due", perform_backup(conn, &dir, app_version, now))
}

/// 触发入口二：退出兜底——只要脏且可用就备份一次，**不受每日约束**。
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
    if !state.dirty {
        // 与「未到期」的静默语义分开表达：退出兜底只关心脏标记。
        return AttemptOutcome::Skipped(SkipReason::Clean);
    }
    classify_result("exit", perform_backup(conn, &dir, app_version, now))
}

/// 触发入口三：首次兜底——启动会话首次拿到目录时，若受管备份列表为空
/// （不分手动/自动）立即备份一次，与脏标记/到期无关（Story 4）。
pub fn run_first_backup(
    conn: &Connection,
    dir: Option<&str>,
    app_version: &str,
    now: DateTime<Utc>,
) -> AttemptOutcome {
    let (_, dir) = match gate(conn, dir) {
        Ok(v) => v,
        Err(outcome) => return outcome,
    };
    match crate::commands::backup::list_managed_backups(Path::new(&dir)) {
        Ok(list) if !list.is_empty() => {
            return AttemptOutcome::Skipped(SkipReason::ListNotEmpty);
        }
        // 目录尚不存在视为空列表：执行兜底时若目录确实无效，由 backup_db_to 报错收场。
        Ok(_) => {}
        Err(e) => return failed(e),
    }
    classify_result("first", perform_backup(conn, &dir, app_version, now))
}

// ---------------------------------------------------------------------------
// 写路径挂钩（issue #126）：所有业务写库成功后置脏 + 写时顺带检查
// ---------------------------------------------------------------------------

/// 进程级共享的偏好镜像单例：写命令深处只有 `&Connection`，拿不到 Tauri 的
/// `State<PrefsState>`，经本单例读取目录镜像，避免把 AppHandle 一路下传到写路径。
static SHARED_PREFS: OnceLock<Arc<PrefsState>> = OnceLock::new();

/// 共享偏好镜像（lib.rs 启动即初始化一次；未显式 `set_dir` 时目录为 None——
/// 所有触发入口静默跳过）。
pub fn shared_prefs() -> Arc<PrefsState> {
    SHARED_PREFS
        .get_or_init(|| Arc::new(PrefsState::default()))
        .clone()
}

/// 业务写库成功后的统一后置动作（issue #126）。
///
/// 1. 置脏：任何业务写库成功后调用（交易写入 Writer 接缝的插入/更新与软删除、
///    参考数据 CRUD、市场数据写入）；失败仅记日志不上抛——业务写已成功，
///    不能因调度状态写入失败回滚用户操作。
/// 2. 写时顺带检查：若到期且脏则立即执行自动备份（决策全在 [`due_decision`]，
///    备份频率上限每天一次；目录未配置/开关关闭等门禁由 [`run_due_backup`] 统一处理）。
///
/// 处于显式事务中（批量导入逐行落库）时只置脏、不做到期检查——VACUUM INTO
/// 不能在事务内执行；提交点（如 [`crate::commands::batch::TransactionBatch::run`]）
/// 会再调一次本函数补上检查。重复调用安全：置脏幂等，未到期检查恒为 Skip。
pub fn on_write(conn: &Connection) {
    if let Err(e) = mark_dirty(conn) {
        tracing::warn!(error = %e, "写库成功但置脏失败（忽略）");
    }
    if !conn.is_autocommit() {
        return;
    }
    let dir = shared_prefs().snapshot_dir();
    run_due_backup(conn, dir.as_deref(), env!("CARGO_PKG_VERSION"), Utc::now());
}

// ---------------------------------------------------------------------------
// 运行时载体：偏好镜像 + 轮询线程 + 退出钩子
// ---------------------------------------------------------------------------

/// 设备本地偏好镜像：`backupDir` 保持前端 localStorage 单一来源（ADR-0016 决策 3），
/// 启动/变更时经 IPC `set_auto_backup_dir` 推送给后端，供调度线程与退出钩子消费。
/// `None` 表示未配置——所有触发入口一律静默跳过。
#[derive(Default)]
pub struct PrefsState {
    /// 当前备份目录镜像。独立 Arc 以便线程与 IPC 两端共享同一份。
    pub dir: Arc<Mutex<Option<String>>>,
    /// 本会话是否已认领「首次兜底」机会（每会话至多一次）。
    first_fallback_claimed: AtomicBool,
}

impl PrefsState {
    fn lock_dir(&self) -> MutexGuard<'_, Option<String>> {
        self.dir.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 更新目录镜像；`None` 表示未配置。
    pub fn set_dir(&self, dir: Option<String>) {
        *self.lock_dir() = dir;
    }

    pub fn snapshot_dir(&self) -> Option<String> {
        self.lock_dir().clone()
    }

    /// 认领本会话的首次兜底机会：返回 true 表示尚未做过、由本次认领。
    pub fn claim_first_fallback(&self) -> bool {
        !self.first_fallback_claimed.swap(true, Ordering::SeqCst)
    }
}

/// 等待连接锁至超时。锁被占用超过 [`LOCK_TIMEOUT`] 或已损坏（poisoned）返回 None，
/// 由调用方跳过本轮并保留脏标记（下个周期重试即重试机制）。
pub(crate) fn lock_conn_with_timeout(
    conn: &Arc<Mutex<Connection>>,
) -> Option<MutexGuard<'_, Connection>> {
    let deadline = Instant::now() + LOCK_TIMEOUT;
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
                        timeout_ms = LOCK_TIMEOUT.as_millis() as u64,
                        "等待数据库锁超时，跳过本轮自动备份"
                    );
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// 启动自动备份轮询线程（标准轮询线程模式：spawn + sleep）。
pub fn start_scheduler(app: &tauri::AppHandle) {
    use tauri::Manager;
    let conn = Arc::clone(&app.state::<DbState>().conn);
    let dir_mirror = Arc::clone(&shared_prefs().dir);
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(CHECK_INTERVAL_SECS));
            let Some(guard) = lock_conn_with_timeout(&conn) else {
                continue; // 拿不到锁：跳过本轮、保留脏标记。
            };
            let version = handle.package_info().version.to_string();
            run_due_backup(
                &guard,
                dir_mirror
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_deref(),
                &version,
                Utc::now(),
            );
        }
    });
}

/// 应用退出兜底入口：挂在 `RunEvent::Exit` 上，退出前若脏则补一次备份
/// （不受每日约束）。拿锁超时只能在日志里留痕——退出后没有下一轮了。
pub fn exit_fallback(app: &tauri::AppHandle) {
    use tauri::Manager;
    let conn = Arc::clone(&app.state::<DbState>().conn);
    let Some(guard) = lock_conn_with_timeout(&conn) else {
        return;
    };
    let dir = shared_prefs().snapshot_dir();
    let version = app.package_info().version.to_string();
    run_exit_backup(&guard, dir.as_deref(), &version, Utc::now());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn conn() -> rusqlite::Connection {
        let mut c = db::open_in_memory().expect("打开内存库");
        db::init_db(&mut c).expect("执行迁移");
        c
    }

    fn ts(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .expect("合法 ISO 时间")
            .with_timezone(&chrono::Utc)
    }

    /// 到期且脏 → BackupNow。
    #[test]
    fn due_and_dirty_means_backup_now() {
        let now = ts("2026-02-17T12:00:00Z");
        assert_eq!(
            due_decision(
                true,
                Some(ts("2026-02-16T11:00:00Z")),
                AUTO_BACKUP_INTERVAL,
                now
            ),
            BackupDecision::BackupNow
        );
    }

    /// 恰好满 24h 也算到期（≥ 间隔）。
    #[test]
    fn exactly_interval_is_due() {
        let now = ts("2026-02-17T12:00:00Z");
        assert_eq!(
            due_decision(
                true,
                Some(ts("2026-02-16T12:00:00Z")),
                AUTO_BACKUP_INTERVAL,
                now
            ),
            BackupDecision::BackupNow
        );
    }

    /// 到期但不脏 → Skip（无变化不重复备份）。
    #[test]
    fn due_but_clean_means_skip() {
        let now = ts("2026-02-17T12:00:00Z");
        assert_eq!(
            due_decision(
                false,
                Some(ts("2026-02-15T00:00:00Z")),
                AUTO_BACKUP_INTERVAL,
                now
            ),
            BackupDecision::Skip
        );
    }

    /// 未到期但脏 → Skip（备份频率上限每天一次）。
    #[test]
    fn fresh_but_dirty_means_skip() {
        let now = ts("2026-02-17T12:00:00Z");
        assert_eq!(
            due_decision(
                true,
                Some(ts("2026-02-17T00:00:00Z")),
                AUTO_BACKUP_INTERVAL,
                now
            ),
            BackupDecision::Skip
        );
    }

    /// 从未备份且脏 → BackupNow（首次有变化即保护）；从未备份也不脏 → Skip。
    #[test]
    fn never_backed_up() {
        let now = ts("2026-02-17T12:00:00Z");
        assert_eq!(
            due_decision(true, None, AUTO_BACKUP_INTERVAL, now),
            BackupDecision::BackupNow
        );
        assert_eq!(
            due_decision(false, None, AUTO_BACKUP_INTERVAL, now),
            BackupDecision::Skip
        );
    }

    /// 状态读写默认值：key 缺失时取约定默认（enabled=true、dirty=false、未备份）。
    #[test]
    fn get_state_defaults_when_keys_missing() {
        let c = conn();
        let state = get_state(&c).expect("读默认状态");
        assert_eq!(state, AutoBackupState::default());
    }

    /// app_settings 表缺失（旧版本备份恢复后）同样返回默认而非报错。
    #[test]
    fn get_state_defaults_when_table_missing() {
        let c = db::open_in_memory().expect("未迁移的内存库");
        let state = get_state(&c).expect("缺表取默认状态");
        assert_eq!(state, AutoBackupState::default());
    }

    /// set_state 写入后 get_state 读回一致。
    #[test]
    fn set_then_get_roundtrip() {
        let c = conn();
        let want = AutoBackupState {
            enabled: false,
            dirty: true,
            last_backup_at: Some(String::from("2026-02-17T08:00:00Z")),
        };
        set_state(&c, &want).expect("写状态");
        assert_eq!(get_state(&c).expect("读回状态"), want);
    }

    /// mark_dirty 置真并可持久化观察到。
    #[test]
    fn mark_dirty_persists() {
        let c = conn();
        mark_dirty(&c).expect("置脏");
        let state = get_state(&c).expect("读状态");
        assert!(state.dirty);
    }

    /// mark_clean 复位脏标记并把当前时间记为上次备份锚点。
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

    /// reset（恢复后重置）：清脏并重新计时，enabled 保持不变。
    #[test]
    fn reset_clears_dirty_and_reanchors() {
        let c = conn();
        set_state(
            &c,
            &AutoBackupState {
                enabled: false,
                dirty: true,
                last_backup_at: Some(String::from("2026-02-10T00:00:00Z")),
            },
        )
        .expect("写脏状态");
        reset(&c, "2026-02-17T10:00:00Z").expect("重置");
        let state = get_state(&c).expect("读状态");
        assert!(!state.dirty);
        assert!(!state.enabled);
        assert_eq!(
            state.last_backup_at,
            Some(String::from("2026-02-17T10:00:00Z"))
        );
    }
    /// on_write（issue #126 写路径挂钩）：置脏；未配置目录时顺带检查静默跳过
    /// （不产生文件、不动锚点）。
    #[test]
    fn on_write_marks_dirty_and_silently_skips_check_without_dir() {
        let c = conn();
        assert!(!get_state(&c).unwrap().dirty);
        crate::auto_backup::on_write(&c);
        let state = get_state(&c).expect("读状态");
        assert!(state.dirty, "写库成功后应置脏");
        assert_eq!(state.last_backup_at, None, "目录未配置不应记录备份锚点");
    }
}

/// T1 调度执行层测试：只测「给定状态 → 正确决定与产物」的外部行为，
/// 不测线程/sleep/锁（时间驱动行为已收进纯函数，线程只做周期调用）。
#[cfg(test)]
mod scheduler_tests {
    use super::*;
    use crate::db;
    use std::fs;
    use std::path::PathBuf;

    fn conn() -> rusqlite::Connection {
        let mut c = db::open_in_memory().expect("打开内存库");
        db::init_db(&mut c).expect("执行迁移");
        c
    }

    /// 与 backup 模块测试同款：临时目录唯一命名，避免并行测试互踩。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ledger-auto-test-{tag}-{}-{}",
            std::process::id(),
            db::new_uuid()
        ));
        fs::create_dir_all(&dir).expect("创建临时目录");
        dir
    }

    fn now_at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .expect("合法 ISO 时间")
            .with_timezone(&Utc)
    }

    #[test]
    fn file_name_follows_naming_rule() {
        assert_eq!(
            auto_backup_file_name(now_at("2026-02-17T09:30:00Z")),
            "ledger-auto-20260217-093000.db.zip"
        );
    }

    /// 到期且脏 → 执行备份：产物按规则命名、脏标记清真并记锚点。
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
                    "ledger-auto-20260217-120000.db.zip"
                );
                // 产物元数据带 auto 来源标记（issue #127）。
                assert_eq!(
                    crate::commands::backup::read_backup_kind(Path::new(path)).unwrap(),
                    crate::commands::backup::BackupKind::Auto
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
        let _ = fs::remove_dir_all(&dir);
    }

    /// 未到期但脏 → 静默跳过，不产生文件、不动状态。
    #[test]
    fn due_backup_skips_when_fresh() {
        let c = conn();
        let dir = temp_dir("due-fresh");
        set_state(
            &c,
            &AutoBackupState {
                enabled: true,
                dirty: true,
                last_backup_at: Some(String::from("2026-02-17T00:00:00Z")),
            },
        )
        .expect("写状态");
        let outcome = run_due_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T12:00:00Z"),
        );
        assert_eq!(outcome, AttemptOutcome::Skipped(SkipReason::NotDue));
        assert!(
            fs::read_dir(&dir).expect("列目录").next().is_none(),
            "不应产生文件"
        );
        assert!(get_state(&c).unwrap().dirty, "跳过不影响状态");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 目录未配置 → 静默跳过（不执行、不报错），脏标记保留等配置后再补。
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

    /// 开关关闭 → 无论多「该备份」都不产生自动备份。
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
        let _ = fs::remove_dir_all(&dir);
    }

    /// 目录已配置但路径无效 → 备份失败归入 Failed 且**保留脏标记**（下周期重试）。
    #[test]
    fn failure_keeps_dirty() {
        let c = conn();
        // 配置了目录镜像但目录本身不存在（父目录也不存在，backup_db_to 必失败）。
        let missing = std::env::temp_dir()
            .join(format!("ledger-auto-missing-{}", db::new_uuid()))
            .join("nested");
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

    /// 退出兜底不受每日约束：刚备份过 1 分钟但数据又变脏也立即备份。
    #[test]
    fn exit_backup_ignores_interval() {
        let c = conn();
        let dir = temp_dir("exit-fresh");
        set_state(
            &c,
            &AutoBackupState {
                enabled: true,
                dirty: true,
                last_backup_at: Some(String::from("2026-02-17T11:59:00Z")),
            },
        )
        .expect("写状态");
        let outcome = run_exit_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T12:00:00Z"),
        );
        assert!(matches!(outcome, AttemptOutcome::Performed { .. }));
        let _ = fs::remove_dir_all(&dir);
    }

    /// 退出兜底：不脏则不备份。
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
        let _ = fs::remove_dir_all(&dir);
    }

    /// 首次兜底：列表为空时即便不脏、从未备份过也立即备份一次；此后列表非空不再兜底
    /// （同时验证 ledger-auto 前缀的产物被受管判定识别）。
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
        let _ = fs::remove_dir_all(&dir);
    }

    /// 首次兜底把手动备份也算进「列表非空」：已有手动产物就不再兜底。
    #[test]
    fn first_fallback_counts_manual_backups() {
        let c = conn();
        let dir = temp_dir("first-manual");
        fs::write(dir.join("ledger-backup-20260101-000000.db.zip"), b"stub")
            .expect("造手动备份占位");
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
        let _ = fs::remove_dir_all(&dir);
    }
}
