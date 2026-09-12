//! 自动备份（AutoBackup）状态、到期判定与调度（issue #124 / #125）。
//!
//! 职责边界：
//! - 状态经 [`crate::settings`] 收口持久化到 `app_settings` KV 表（`auto_backup.*` key），
//!   不建专表；IPC 由 `commands::backup` 的领域命令形状暴露（issue #128 接前端）；
//! - 到期判定为纯函数 [`due_decision`]，当前时间由调用方注入，不依赖全局时钟——
//!   **线程只做周期调用，是否备份的决策全在纯函数**（可测边界）；「今天」由注入
//!   时刻换算本地时区日期，备份频率上限为本地自然日每天最多一次（issue #386）；
//! - 调度层（issue #125）提供三种触发入口（周期到期 / 退出兜底 / 首次兜底），
//!   全部收敛到同一执行函数 [`perform_backup`]；轮询线程为标准模式
//!   （spawn + sleep），锁等待带超时，超时跳过本轮；
//! - `backupDir` 保持前端 localStorage 单一来源（ADR-0016 决策 3），后端只维护
//!   一份运行时镜像 [`PrefsState`]（启动时经 IPC `set_auto_backup_dir` 推送），
//!   目录未配置时一律静默跳过；
//! - 置脏触发（issue #126 的「写时顺带检查」）已整体迁入连接层统一写入口
//!   [`crate::db::write`]（ADR-0032，#246 收口）：本模块不再暴露写路径挂钩，
//!   只保留域原语 [`mark_dirty`]（`pub(crate)`）与触发入口 [`run_due_backup`]
//!   （BackupTrigger 接口面，运行时仅调度线程与连接层提交点调用）供连接层
//!   提交点组合；深度模块只持有 `&Connection`，不经 AppHandle 取偏好——
//!   目录镜像经进程级单例 [`shared_prefs`] 读取，应用版本用编译期常量。
//!
//! 备注：`SettingKey::AutoBackupNextDueAt` 已预留但本模块不读写——日界模型下
//! 「下次到期时间」由本地日期比较即时得出，不落地派生数据。

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Offset, TimeZone, Utc};
use rusqlite::Connection;

use crate::db::{self, DbState};
use crate::settings::{self, SettingKey};

/// 到期判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupDecision {
    /// 到期，应立即执行自动备份。
    BackupNow,
    /// 数据无变化（不脏），无需备份。
    Clean,
    /// 今天（本地自然日）已自动备份过——含时钟回拨（锚点日期晚于今天）的安全侧。
    AlreadyBackedUpToday,
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
/// 语义（ADR-0016 修订，issue #386）：脏 且（从未备份 或 上次自动备份的本地日期
/// 早于今天）→ [`BackupDecision::BackupNow`]；不脏 → [`BackupDecision::Clean`]；
/// 脏但今天已备过 → [`BackupDecision::AlreadyBackedUpToday`]。备份频率上限为
/// 本地自然日每天最多一次；「今天」取 `now_local`（调用方注入时刻的本地时区视角）
/// 的日期，与定时追补、订阅花费总览同口径；锚点沿用「上次自动备份时刻」
/// （UTC 存储、值格式不变），判定时换算本地日期，不新增设置键、不迁移数据。
/// 当前时刻由调用方注入，不依赖全局时钟——**线程只做周期调用，是否备份的决策全在纯函数**。
///
/// 边界约定：
/// - 首次兜底（备份列表为空即备份一次）与脏标记无关，其日界门直接用 [`backed_up_today`]；
/// - 锚点的本地日期晚于今天（时钟回拨）按「今天已备过」安全侧跳过，随时间推移自然恢复。
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

/// 日界门原语（三入口统一）：上次自动备份时刻换算本地日期是否不早于「今天」。
/// 从未备份（`None`）视为今天还没备过；锚点日期晚于今天（时钟回拨）按已备安全侧。
///
/// 换算用「今天」的时区偏移（非锚点时刻的历史偏移）：保证判定表单测与进程时区
/// 无关（唯一注入点是 `now_local`）。已知取舍：夏令时时区在时制切换日，锚点
/// 换算日期与真实本地日期可能有 ±1 小时级的边界偏差（回拨日极端情况下同日
/// 可重复一次，提前日则多跳过一天）；中国主时区无夏令时不受影响，偏差次日自愈。
fn backed_up_today<Tz: TimeZone>(
    last_backup_at: Option<DateTime<Utc>>,
    now_local: DateTime<Tz>,
) -> bool {
    match last_backup_at {
        None => false,
        // 锚点按「今天」的时区偏移换算本地日期（fix() 取固定偏移量，比较只看日期）。
        Some(last) => {
            let offset = now_local.offset().fix();
            last.with_timezone(&offset).date_naive() >= now_local.date_naive()
        }
    }
}

/// 解析调度状态里的备份锚点（UTC ISO → UTC 时刻）；解析失败按「从未备份」处理。
fn anchor_of(state: &AutoBackupState) -> Option<DateTime<Utc>> {
    state.last_backup_at.as_deref().and_then(parse_iso)
}

/// 调用方注入的 UTC 时刻换算本地时区视角（日界判定用，「今天」的唯一换算点）。
fn to_local(now: DateTime<Utc>) -> DateTime<Local> {
    now.with_timezone(&Local)
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

/// 置脏：业务写库成功后由连接层统一写入口在提交点调用（ADR-0032）。
/// 私有表示它不是业务代码的调用点——业务写经 [`crate::db::write`]，置脏是其
/// 结构性副作用；暴露面只有接线用钩子（[`after_commit_hook`] /
/// [`occurrence_dirty_hook`]）。
fn mark_dirty(conn: &Connection) -> crate::error::Result<()> {
    settings::set(conn, SettingKey::AutoBackupDirty, &true)
}

/// 置脏并忽略失败（提交点钩子与期次落账钩子的共享落点）：置脏失败仅记日志
/// 不上抛——引发置脏的写/落账已成功，不因置脏失败回滚或报错。
fn mark_dirty_ignoring_failure(conn: &Connection) {
    if let Err(e) = mark_dirty(conn) {
        tracing::warn!(error = %e, "写库成功但置脏失败（忽略）");
    }
}

/// 连接层提交点后置动作实现（ADR-0032 置脏单点，原 `db::after_commit` 本体）：
/// 基础设施侧只保留注册点与调用时机（issue #1088 crate 归位后数据库不再知道
/// 备份域），副作用语义住本域。公开面仅为「接线用」——壳层启动、测试建库单点与
/// 基础设施 crate 的单测各自把它注册进去（dev-dependency 环下 crate 实例分离，
/// 见 [`install_after_commit_hook`]），业务代码不直接调用。
///
/// - 置脏失败仅记日志不上抛，不影响已成功的业务写；
/// - 到期检查命中（脏且今天尚未自动备份，本地自然日界门，issue #386）即执行自动
///   备份；开关关闭/目录未配置等门禁由 [`run_due_backup`] 统一静默处理。
pub fn after_commit_hook(conn: &Connection) {
    mark_dirty_ignoring_failure(conn);
    let dir = shared_prefs().snapshot_dir();
    // 备份作用域从偏好镜像快照（引导登记点播种，issue #836）：写路径深处只有
    // `&Connection`，账本归属经镜像统一承载，与调度线程同一来源。
    let scope = shared_prefs().snapshot_scope();
    run_due_backup(
        conn,
        dir.as_deref(),
        env!("CARGO_PKG_VERSION"),
        Utc::now(),
        scope.as_ref(),
    );
}

/// 把提交点后置动作注册进连接层写入口（幂等，进程级一次）。
///
/// 接缝形态（spec #1086 / issue #1088）：下层（基础设施）定义注册点、上层（本域）
/// 提供实现、壳层启动时接线——`lib.rs::run` 与测试建库单点（`test_support::open`、
/// BDD world）各调用一次，生产与测试同形。基础设施 crate 的单测经
/// dev-dependency 环拿到的是**另一份** crate 实例（静态与类型身份分离），故它直接
/// 以 [`after_commit_hook`] 注册进自己那份写入口静态，不经本函数。
pub fn install_after_commit_hook() {
    db::register_after_commit_hook(after_commit_hook);
}

/// 定时追补落账的置脏钩子实现（issue #1090 / spec #1086 形态推广）：备份域提供
/// 实现、壳层启动时注册进定时计划域的注册点
/// （`scheduled_transactions::auto_run::register_after_occurrence_hook`）——定时
/// 计划域对备份域零直接依赖。置脏失败仅记日志不上抛（与提交点钩子同款：落账
/// 已成功，不因置脏失败报错）。
pub fn occurrence_dirty_hook(conn: &Connection) {
    mark_dirty_ignoring_failure(conn);
}

/// 把期次落账置脏实现注册进定时计划域的注册点（幂等，进程级一次）。
///
/// 接缝形态（issue #1090 / spec #1086）：调用点在壳层启动接线与测试建库单点
/// （`test_support::open`、BDD world），与生产同形。
pub fn install_occurrence_dirty_hook() {
    crate::scheduled_transactions::auto_run::register_after_occurrence_hook(occurrence_dirty_hook);
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

/// 自动备份产物命名前缀：`ledger-auto-YYYYMMDD-HHMMSS.db.zip`，
/// 时间戳取本地时间（ADR-0016 修订：原 UTC，与手动备份命名拉齐；
/// 存量 UTC 命名产物不迁移，随滚动清理自然淘汰）。
/// 后端受管备份判定（域内 `engine` 受管命名规则）与前端命名/判定共用该语义（T3）。
pub const AUTO_BACKUP_PREFIX: &str = "ledger-auto-";

/// 轮询检查周期：每 10 分钟醒来检查一次到期判定（ADR-0016 及其修订注记：原 30 分钟）。
const CHECK_INTERVAL_SECS: u64 = 10 * 60;
/// 执行备份时等待 DB 连接锁的超时；超时跳过本轮、保留脏标记，下个周期重试。
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// 自动备份产物文件名：`ledger-auto-YYYYMMDD-HHMMSS[-<账本标识>].db.zip`。
/// 时间戳按 `now` 自身时区渲染，纯函数不做任何时区换算——运行时传注入时刻的
/// 本地时间（ADR-0016 修订：原 UTC），测试以固定偏移注入即可对运行机器时区稳定。
/// 账本标识（issue #836）：`Some` 时携带，两本账的自动产物互不覆盖；格式
/// 与手动备份同源（[`super::engine::managed_backup_file_name`]）。
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
    /// 数据无变化（不脏），不满足到期入口的触发条件。
    NotDue,
    /// 今天（本地自然日）已自动备份过：先到先触发的日界门，三入口统一（issue #386；
    /// 含时钟回拨锚点日期晚于今天的安全侧）。
    AlreadyBackedUpToday,
    /// 退出兜底专属：数据无变化（不脏），无事可做。
    Clean,
    /// 首次兜底专属：备份列表非空（不分手动/自动），无需兜底。
    ListNotEmpty,
}

/// 唯一执行落点：生成自动命名产物 + 成功后 [`mark_clean`]。
/// 失败时不上抛由调用方归入 [`AttemptOutcome::Failed`]——脏标记自然保留。
/// `scope`：账本作用域（issue #836），产物名携带其账本标识；`None` = 旧命名。
fn perform_backup(
    conn: &Connection,
    dir: &str,
    app_version: &str,
    now: DateTime<Utc>,
    scope: Option<&super::engine::BackupScope>,
) -> crate::error::Result<String> {
    // 产物命名取注入时刻的本地时间（ADR-0016 修订：原 UTC，与手动备份拉齐）；
    // 锚点仍记 UTC 时刻（[`db::iso_at`]），值格式不变。
    let target = Path::new(dir).join(auto_backup_file_name(
        now.with_timezone(&Local),
        scope.map(super::engine::BackupScope::book_tag),
    ));
    let path =
        super::engine::backup_db_to(conn, &target, app_version, super::engine::BackupKind::Auto)?
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

/// 触发入口一：周期到期尝试。运行时调用方仅两处：轮询调度线程与连接层写入口
/// 提交点的写时顺带检查（ADR-0032）；对外保留 `pub` 供 e2e 测试驱动触发（BackupTrigger
/// 接口面，ADR-0016）。读状态 → [`due_decision`] 纯函数决策 → 命中则执行。
pub fn run_due_backup(
    conn: &Connection,
    dir: Option<&str>,
    app_version: &str,
    now: DateTime<Utc>,
    scope: Option<&super::engine::BackupScope>,
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
    classify_result("due", perform_backup(conn, &dir, app_version, now, scope))
}

/// 触发入口二：退出兜底——脏且当天尚未自动备份过才补一次，与到期入口同受
/// 统一日界门约束（issue #386：原「不受每日约束」的豁免已取消）。
pub fn run_exit_backup(
    conn: &Connection,
    dir: Option<&str>,
    app_version: &str,
    now: DateTime<Utc>,
    scope: Option<&super::engine::BackupScope>,
) -> AttemptOutcome {
    let (state, dir) = match gate(conn, dir) {
        Ok(v) => v,
        Err(outcome) => return outcome,
    };
    let last = anchor_of(&state);
    match due_decision(state.dirty, last, to_local(now)) {
        BackupDecision::BackupNow => {}
        // 与到期入口的「未到期」分开表达：退出兜底只关心脏标记。
        BackupDecision::Clean => return AttemptOutcome::Skipped(SkipReason::Clean),
        BackupDecision::AlreadyBackedUpToday => {
            tracing::debug!(trigger = "exit", "今天已自动备份，日界门静默跳过");
            return AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday);
        }
    }
    classify_result("exit", perform_backup(conn, &dir, app_version, now, scope))
}

/// 触发入口三：首次兜底——启动会话首次拿到目录时，若受管备份列表为空
/// （不分手动/自动）立即备份一次，与脏标记无关，但同受日界门约束
/// （Story 4 / issue #386：当天已备过就不再重复）。
pub fn run_first_backup(
    conn: &Connection,
    dir: Option<&str>,
    app_version: &str,
    now: DateTime<Utc>,
    scope: Option<&super::engine::BackupScope>,
) -> AttemptOutcome {
    let (state, dir) = match gate(conn, dir) {
        Ok(v) => v,
        Err(outcome) => return outcome,
    };
    // 「列表为空」按账本作用域判定（issue #836）：新账本首次进入即有自己
    // 的首次兜底，不被其他账本的存量产物挡住。
    match super::engine::list_managed_backups(Path::new(&dir), scope) {
        Ok(list) if !list.is_empty() => {
            return AttemptOutcome::Skipped(SkipReason::ListNotEmpty);
        }
        // 目录尚不存在视为空列表：执行兜底时若目录确实无效，由 backup_db_to 报错收场。
        Ok(_) => {}
        Err(e) => return failed(e),
    }
    let last = anchor_of(&state);
    if backed_up_today(last, to_local(now)) {
        tracing::debug!(trigger = "first", "今天已自动备份，日界门静默跳过首次兜底");
        return AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday);
    }
    classify_result("first", perform_backup(conn, &dir, app_version, now, scope))
}

// ---------------------------------------------------------------------------
// 进程级偏好镜像：连接层写入口提交点检查与轮询线程共享（ADR-0032）
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
    /// 当前活动账本的备份作用域镜像（issue #836）：引导登记时由壳层播种
    /// （[`seed_book_scope`]），供调度线程、连接层写入口提交点与退出兜底
    /// 统一消费——连接层深处只有 `&Connection`，拿不到引导状态。
    scope: Mutex<Option<super::engine::BackupScope>>,
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

    fn lock_scope(&self) -> MutexGuard<'_, Option<super::engine::BackupScope>> {
        self.scope.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 快照当前备份作用域（`None` = 注册表不可用/未播种，按旧命名兼容口径）。
    pub fn snapshot_scope(&self) -> Option<super::engine::BackupScope> {
        self.lock_scope().clone()
    }
}

/// 播种账本备份作用域（引导登记点调用，issue #836）：由注册表登记信息构造
/// 当前活动账本的作用域；注册表损坏（`None`）时清空镜像——回退默认目录建连
/// 的现场无从归属账本，自动备份退化为旧命名兼容口径（产物仍可靠，仅命名
/// 不携带账本标识，待注册表修复后自然恢复分域）。
pub fn seed_book_scope(registry: Option<&crate::db::book_registry::BookRegistry>) {
    let scope = registry.map(super::engine::BackupScope::of_registry);
    *shared_prefs().lock_scope() = scope;
    tracing::debug!(
        seeded = shared_prefs().snapshot_scope().is_some(),
        "备份作用域已播种"
    );
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

/// 调度线程单次拉起守卫（issue #644 / ADR-0080）：启动、解锁恢复、原位重
/// 引导三个入口收敛为「进程内至多一个调度线程」——线程持共享连接 Arc，
/// 重引导换连对它透明；锁定/失败期间由每轮门检空转，不重建线程。
static SCHEDULER_SPAWNED: AtomicBool = AtomicBool::new(false);

/// 启动调度轮询线程（标准轮询线程模式：spawn + sleep）。
///
/// 单一 tick 双判定（issue #307 / ADR-0042）：每轮先做自动备份到期判定，再做
/// 定时计划追补判定；线程只做周期调用——备份决策全在纯函数到期判定，追补决策
/// 全在参数注入（连接、开关状态、今天日期）的追补入口（开关从运行时镜像读出）。
///
/// 幂等（issue #644 / ADR-0080）：已在跑时本调用退化为无操作；每轮门检在
/// 锁定/启动失败期间跳过备份与追补（占位连接不是业务库）——原位重引导把
/// 门翻回锁定后，已存活的线程在门检处等待，解锁/恢复后自动继续。
pub fn start_scheduler(app: &tauri::AppHandle) {
    use tauri::Manager;
    if SCHEDULER_SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let conn = Arc::clone(&app.state::<DbState>().conn);
    let gate = crate::db::encryption::EncryptionGate::clone(
        &app.state::<crate::db::encryption::EncryptionGate>(),
    );
    let boot_gate =
        crate::db::boot::BootFailureGate::clone(&app.state::<crate::db::boot::BootFailureGate>());
    let dir_mirror = Arc::clone(&shared_prefs().dir);
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(CHECK_INTERVAL_SECS));
            // 每轮门检（issue #644 / ADR-0080）：锁定/启动失败期间不触碰占位连接。
            if gate.is_locked() || boot_gate.is_failed() {
                continue;
            }
            let Some(guard) = lock_conn_with_timeout(&conn) else {
                continue; // 拿不到锁：跳过本轮、保留脏标记。
            };
            let version = handle.package_info().version.to_string();
            // 备份作用域从偏好镜像快照（引导登记点播种，issue #836）：切换账本
            // 经原位重引导后镜像随之更新，调度线程无需重建。
            let scope = shared_prefs().snapshot_scope();
            run_due_backup(
                &guard,
                dir_mirror
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_deref(),
                &version,
                Utc::now(),
                scope.as_ref(),
            );
            // 追补判定（issue #307 / ADR-0042）：开关从镜像读出后注入追补入口，
            // 今天取本地时区日期（与订阅花费总览同款口径）；镜像默认关，未推送即空转。
            crate::scheduled_transactions::run_catch_up(
                &guard,
                crate::scheduled_transactions::auto_run::is_enabled(),
                chrono::Local::now().date_naive(),
            );
        }
    });
}

/// 应用退出兜底入口：挂在 `RunEvent::Exit` 上，退出前若脏且当天尚未自动备份过
/// 则补一次备份（与到期入口同受日界门约束，issue #386）。拿锁超时只能在日志里留痕
/// ——退出后没有下一轮了。
pub fn exit_fallback(app: &tauri::AppHandle) {
    use tauri::Manager;
    let conn = Arc::clone(&app.state::<DbState>().conn);
    let Some(guard) = lock_conn_with_timeout(&conn) else {
        return;
    };
    let dir = shared_prefs().snapshot_dir();
    let scope = shared_prefs().snapshot_scope();
    let version = app.package_info().version.to_string();
    run_exit_backup(&guard, dir.as_deref(), &version, Utc::now(), scope.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> rusqlite::Connection {
        // 建库两行序经统一测试工厂承载（spec #728 / issue #758 / ADR-0084 决策 3/7）。
        crate::test_support::open()
    }

    fn ts(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .expect("合法 ISO 时间")
            .with_timezone(&chrono::Utc)
    }

    /// 判定表（issue #386）：固定东八区作为「本地时区」注入，判定表与进程时区无关、
    /// 覆盖首次 / 未脏 / 当天已备 / 昨日已备 / 时钟回拨五类。各用例经 `local_at`
    /// 构造带偏移的时刻，锚点用 UTC 存储形态。
    mod decision_table {
        use super::*;
        use chrono::FixedOffset;

        fn local_at(s: &str) -> DateTime<FixedOffset> {
            DateTime::parse_from_rfc3339(s).expect("合法 ISO 时间")
        }

        /// 首次：脏 + 从未备份 → BackupNow（首次有变化即保护）；不脏 → Clean。
        #[test]
        fn first_backup_when_never_backed_up() {
            let now = local_at("2026-02-17T10:00:00+08:00");
            assert_eq!(due_decision(true, None, now), BackupDecision::BackupNow);
            assert_eq!(due_decision(false, None, now), BackupDecision::Clean);
        }

        /// 未脏：数据无变化，无论锚点多旧都不备份。
        #[test]
        fn clean_means_no_backup() {
            let now = local_at("2026-02-17T10:00:00+08:00");
            assert_eq!(
                due_decision(false, Some(ts("2026-02-15T00:00:00Z")), now),
                BackupDecision::Clean
            );
        }

        /// 当天已备：锚点本地日期 == 今天 → AlreadyBackedUpToday（先到先触发，
        /// 当天不再第二次；含本地清晨备份、深夜再触发的跨 UTC 日情况与恰同时刻边界）。
        #[test]
        fn already_backed_up_today_skips() {
            let now = local_at("2026-02-17T23:45:00+08:00");
            // 本地 17 日 00:15 备份（UTC 16 日 16:15），同一本地日深夜再触发。
            let anchor = ts("2026-02-16T16:15:00Z");
            assert_eq!(
                due_decision(true, Some(anchor), now),
                BackupDecision::AlreadyBackedUpToday
            );
            // 边界：锚点恰为「现在」同样视为已备。
            assert_eq!(
                due_decision(true, Some(now.with_timezone(&Utc)), now),
                BackupDecision::AlreadyBackedUpToday
            );
        }

        /// 昨日已备：锚点本地日期早于今天 → BackupNow（跨本地日恢复备份）。
        #[test]
        fn backed_up_yesterday_is_due_again() {
            let now = local_at("2026-02-18T00:30:00+08:00");
            // 本地 17 日 23:30 备份（UTC 15:30），跨过本地午夜 1 小时后即恢复。
            let anchor = ts("2026-02-17T15:30:00Z");
            assert_eq!(
                due_decision(true, Some(anchor), now),
                BackupDecision::BackupNow
            );
        }

        /// 时钟回拨：锚点本地日期晚于今天按「刚备份过」安全侧跳过。
        #[test]
        fn clock_rollback_is_safe_side_skip() {
            let now = local_at("2026-02-17T10:00:00+08:00");
            // 锚点为本地 18 日 10:00（UTC 02:00）——晚于「今天」。
            let anchor = ts("2026-02-18T02:00:00Z");
            assert_eq!(
                due_decision(true, Some(anchor), now),
                BackupDecision::AlreadyBackedUpToday
            );
        }
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
        // 工厂库删除 app_settings 模拟旧版本备份恢复后的缺表现场（工厂无
        // 「未迁移库」形态：建库 = 内存库 + 迁移，ADR-0084 决策 3）。
        let c = crate::test_support::open();
        c.execute("DROP TABLE app_settings", [])
            .expect("删除 app_settings");
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
    // 原写路径挂钩行为（置脏 + 目录未配置静默跳过不记锚点）已随 ADR-0032 迁入
    // 连接层写入口，由 db::tests::dirty_marker::write_ok_marks_dirty 在写入口层面钉住。
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
        // 建库两行序经统一测试工厂承载（spec #728 / issue #758 / ADR-0084 决策 3/7）。
        crate::test_support::open()
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

    /// 以进程本地时区构造墙上时刻再换 UTC：与触发入口内部的本地换算同一坐标系，
    /// 「同本地日 / 跨本地日」的相对语义与进程时区无关地成立。
    fn local_utc(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> DateTime<Utc> {
        use chrono::TimeZone;
        Local
            .with_ymd_and_hms(y, m, d, hh, mm, 0)
            .single()
            .expect("无歧义本地时刻")
            .with_timezone(&Utc)
    }

    /// 以进程本地时区构造「days_ago 天前」的本地正午再换 UTC（正午不落时制切换
    /// 窗口，跨本地日测试用；days_ago=0 即今天正午）。
    fn local_noon_days_ago_utc(days_ago: u64) -> DateTime<Utc> {
        use chrono::TimeZone;
        let date = Local::now().date_naive() - chrono::Days::new(days_ago);
        Local
            .from_local_datetime(&date.and_hms_opt(12, 0, 0).unwrap())
            .earliest()
            .expect("本地正午可解析")
            .with_timezone(&Utc)
    }

    /// 命名时间戳取注入时刻的**本地时间**（ADR-0016 修订：原 UTC）。
    /// 以固定偏移（UTC+8）注入断言，不依赖运行机器时区，CI 与本地不漂移。
    #[test]
    fn file_name_uses_local_time_of_injected_instant() {
        let tz = chrono::FixedOffset::east_opt(8 * 3600).expect("固定偏移");
        // UTC 09:30 → 本地当天 17:30：同一时刻的文件名时间戳应为本地渲染。
        assert_eq!(
            auto_backup_file_name(now_at("2026-02-17T09:30:00Z").with_timezone(&tz), None),
            "ledger-auto-20260217-173000.db.zip"
        );
        // 跨日边界：UTC 16:30 → 本地次日 00:30，本地日期进位。
        assert_eq!(
            auto_backup_file_name(now_at("2026-02-17T16:30:00Z").with_timezone(&tz), None),
            "ledger-auto-20260218-003000.db.zip"
        );
        // 携带账本标识（issue #836）：两本账的产物互不覆盖；标识位于时间戳之后。
        assert_eq!(
            auto_backup_file_name(
                now_at("2026-02-17T09:30:00Z").with_timezone(&tz),
                Some("book-a")
            ),
            "ledger-auto-20260217-173000-book-a.db.zip"
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
            None,
        );
        match &outcome {
            AttemptOutcome::Performed { path } => {
                assert!(Path::new(path).is_file(), "产物文件应存在");
                // 产物文件名时间戳 = 注入时刻的本地时间渲染（ADR-0016 修订：原 UTC）；
                // 断言用 Local 反推期望值，对运行机器时区稳定。
                assert_eq!(
                    Path::new(path).file_name().unwrap().to_str().unwrap(),
                    format!(
                        "ledger-auto-{}.db.zip",
                        now_at("2026-02-17T12:00:00Z")
                            .with_timezone(&Local)
                            .format("%Y%m%d-%H%M%S")
                    )
                );
                // 产物元数据带 auto 来源标记（issue #127）。
                assert_eq!(
                    crate::backup::read_backup_kind(Path::new(path)).unwrap(),
                    crate::backup::BackupKind::Auto
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

    /// 同一本地日已自动备份过（本地 08:00 锚点、20:00 再触发）→ 静默跳过且原因可辨：
    /// 不产生文件、不前移锚点、脏标记保留（当天改动次日第一个触发点补上）。
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
            },
        )
        .expect("写状态");
        let outcome = run_due_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            local_utc(2026, 2, 17, 20, 0),
            None,
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
        let _ = fs::remove_dir_all(&dir);
    }

    /// 跨本地日恢复备份：锚点拨到本地昨天、今天脏 → 再次执行并前移锚点。
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
            },
        )
        .expect("写状态");
        let now = local_noon_days_ago_utc(0);
        let outcome = run_due_backup(&c, Some(dir.to_str().unwrap()), "0.2.0", now, None);
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
        let _ = fs::remove_dir_all(&dir);
    }

    /// 备份失败保留脏标记后，同日下个触发点重试仍然可行（锚点未记 → 日界门放行）。
    #[test]
    fn failure_retry_same_day_succeeds() {
        let c = conn();
        let dir = temp_dir("failure-retry");
        mark_dirty(&c).expect("置脏");
        // 第一次：目录无效（父目录也不存在）→ Failed，脏保留、锚点不记。
        let missing = std::env::temp_dir()
            .join(format!("ledger-auto-missing-{}", db::new_uuid()))
            .join("nested");
        let first = run_due_backup(
            &c,
            Some(missing.to_str().unwrap()),
            "0.2.0",
            Utc::now(),
            None,
        );
        assert!(
            matches!(first, AttemptOutcome::Failed { .. }),
            "实际 {first:?}"
        );
        assert!(get_state(&c).unwrap().dirty, "失败必须保留脏标记");
        // 第二次（同日，锚点未记）：换有效目录 → 重试成功。
        let second = run_due_backup(&c, Some(dir.to_str().unwrap()), "0.2.0", Utc::now(), None);
        assert!(
            matches!(second, AttemptOutcome::Performed { .. }),
            "同日重试应可行，实际 {second:?}"
        );
        let state = get_state(&c).expect("读状态");
        assert!(
            !state.dirty && state.last_backup_at.is_some(),
            "重试成功后清真并记锚点"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 目录未配置 → 静默跳过（不执行、不报错），脏标记保留等配置后再补。
    #[test]
    fn due_backup_silent_skip_without_dir() {
        let c = conn();
        mark_dirty(&c).expect("置脏");
        for dir in [None, Some(""), Some("   ")] {
            let outcome = run_due_backup(&c, dir, "0.2.0", now_at("2026-02-17T12:00:00Z"), None);
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
                now_at("2026-02-17T12:00:00Z"),
                None,
            ),
            AttemptOutcome::Skipped(SkipReason::Disabled)
        );
        assert_eq!(
            run_exit_backup(
                &c,
                Some(dir.to_str().unwrap()),
                "0.2.0",
                now_at("2026-02-17T12:00:00Z"),
                None,
            ),
            AttemptOutcome::Skipped(SkipReason::Disabled)
        );
        assert_eq!(
            run_first_backup(
                &c,
                Some(dir.to_str().unwrap()),
                "0.2.0",
                now_at("2026-02-17T12:00:00Z"),
                None,
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
            None,
        );
        assert!(
            matches!(outcome, AttemptOutcome::Failed { .. }),
            "实际 {outcome:?}"
        );
        let state = get_state(&c).unwrap();
        assert!(state.dirty, "失败必须保留脏标记");
        assert_eq!(state.last_backup_at, None);
    }

    /// 退出兜底同受日界门（issue #386，原「不受每日约束」豁免取消）：
    /// 当天已自动备份过，退出时即使脏也静默跳过。
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
            },
        )
        .expect("写状态");
        let outcome = run_exit_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            local_utc(2026, 2, 17, 20, 0),
            None,
        );
        assert_eq!(
            outcome,
            AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday)
        );
        assert!(fs::read_dir(&dir).expect("列目录").next().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 退出兜底：当天尚未备份且脏 → 备份（晚间记账后退出不裸奔过夜）。
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
            },
        )
        .expect("写状态");
        let outcome = run_exit_backup(&c, Some(dir.to_str().unwrap()), "0.2.0", Utc::now(), None);
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
            None,
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
        let first = run_first_backup(&c, Some(d), "0.2.0", now_at("2026-02-17T08:00:00Z"), None);
        assert!(
            matches!(first, AttemptOutcome::Performed { .. }),
            "首次应兜底，实际 {first:?}"
        );
        let again = run_first_backup(&c, Some(d), "0.2.0", now_at("2026-02-17T09:00:00Z"), None);
        assert_eq!(again, AttemptOutcome::Skipped(SkipReason::ListNotEmpty));
        let _ = fs::remove_dir_all(&dir);
    }

    /// 首次兜底同受日界门（issue #386）：列表为空但当天已自动备份过（目录被清空）
    /// → 静默跳过、不产生文件，「每天一个」不被首次兜底绕过。
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
            },
        )
        .expect("写状态");
        let outcome = run_first_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            local_utc(2026, 2, 17, 20, 0),
            None,
        );
        assert_eq!(
            outcome,
            AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday)
        );
        assert!(fs::read_dir(&dir).expect("列目录").next().is_none());
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
            None,
        );
        assert_eq!(outcome, AttemptOutcome::Skipped(SkipReason::ListNotEmpty));
        assert!(
            fs::read_dir(&dir).expect("列目录").count() == 1,
            "不应新增文件"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

/// T5 作用域追加测试：首次兜底按账本作用域判定（issue #836）——新账本首次
/// 进入即有自己的首次兜底，不被其他账本的存量产物挡住。
#[cfg(test)]
mod book_scope_tests {
    use super::*;
    use crate::backup::BackupScope;
    use std::fs;
    use std::path::PathBuf;

    fn conn() -> rusqlite::Connection {
        crate::test_support::open()
    }

    fn now_at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .expect("合法 ISO 时间")
            .with_timezone(&Utc)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ledger-auto-scope-{tag}-{}-{}",
            std::process::id(),
            db::new_uuid()
        ));
        fs::create_dir_all(&dir).expect("创建临时目录");
        dir
    }

    /// 目录里已有账本 A 的产物：账本 B 的首次兜底仍执行（按本判定列表为空），
    /// 产物命名携带 B 的标识，互不覆盖。
    #[test]
    fn first_fallback_is_scoped_per_book() {
        let c = conn();
        let dir = temp_dir("per-book");
        fs::write(
            dir.join("ledger-auto-20260101-000000-book-a.db.zip"),
            b"stub",
        )
        .expect("造 A 的存量产物");
        let scope_b = BackupScope {
            book_id: "book-b".into(),
            include_legacy: false,
        };
        let outcome = run_first_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T08:00:00Z"),
            Some(&scope_b),
        );
        match &outcome {
            AttemptOutcome::Performed { path } => {
                let name = Path::new(path).file_name().unwrap().to_str().unwrap();
                // 时间戳取注入时刻的本地时间渲染：期望值用 Local 反推，
                // 对运行机器时区稳定（CI runner 为 UTC，不漂移）。
                let expected = format!(
                    "ledger-auto-{}-book-b.db.zip",
                    now_at("2026-02-17T08:00:00Z")
                        .with_timezone(&Local)
                        .format("%Y%m%d-%H%M%S")
                );
                assert_eq!(name, expected, "产物命名按本地时间渲染并携带账本标识");
            }
            other => panic!("账本 B 应有自己的首次兜底，实际 {other:?}"),
        }
        // 再次兜底：B 的列表非空，跳过。
        let again = run_first_backup(
            &c,
            Some(dir.to_str().unwrap()),
            "0.2.0",
            now_at("2026-02-17T09:00:00Z"),
            Some(&scope_b),
        );
        assert_eq!(again, AttemptOutcome::Skipped(SkipReason::ListNotEmpty));
        let _ = fs::remove_dir_all(&dir);
    }
}
