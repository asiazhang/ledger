//! 自动备份（AutoBackup）状态与到期判定（issue #124）。
//!
//! 职责边界：本模块只负责「状态存取」与「到期判定」两件事——
//! - 状态经 [`crate::settings`] 收口持久化到 `app_settings` KV 表（`auto_backup.*` key），
//!   不建专表、不暴露 IPC（后续 ticket 以领域命令形状提供）；
//! - 到期判定为纯函数，当前时间由调用方注入，不依赖全局时钟，方便测试。
//!
//! 备注：`SettingKey::AutoBackupNextDueAt` 已预留但本模块不读写——锚点模型下
//! 「下次到期时间」可由 `last_backup_at + 间隔` 推导，不落地派生数据；
//! 后续调度 ticket 若需直接展示/持久化该值再接入。

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::Connection;

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
}
