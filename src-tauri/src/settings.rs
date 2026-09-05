//! 应用配置 KV 归口（ADR-0017，issue #130）。
//!
//! 后端权威的配置与运行时状态统一存 `app_settings` KV 表，key 以
//! `<feature>.<name>` 点分命名、由 [`SettingKey`] 枚举集中定义；值用
//! serde_json 序列化、类型由读取方声明。本模块不暴露任何 IPC 命令，
//! 对外接口由后续 ticket 以领域命令形状提供。
//!
//! **置脏豁免（ADR-0032）**：设置与调度状态写入不算「账本数据变化」，
//! `app_settings` 全表写（即本模块的 [`set`]）不置脏——本模块不经连接层
//! 统一写入口 [`crate::db::write`]，调用方持普通锁写入即可；豁免清单
//! 集中在本模块单点，勿为绕开置脏在业务路径直写 `app_settings`。

use rusqlite::Connection;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// app_settings 建表语句（与迁移 V008 同源，CREATE TABLE IF NOT EXISTS 幂等），
/// 供「表缺失自愈」兑底复用。
const APP_SETTINGS_SQL: &str = include_str!("../migrations/V008__app_settings.sql");

/// 配置键枚举：唯一合法的 `app_settings.key` 来源，杜绝字符串字面量散落。
/// 命名规范 `<feature>.<name>`。新增配置项的成本是加一个变体 + 默认值约定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
    /// 自动备份开关（bool，默认 true）。
    AutoBackupEnabled,
    /// 备份脏标记：数据变动后置 true，备份完成后复位（bool，默认 false）。
    AutoBackupDirty,
    /// 上次成功备份时间（Option<String>，UTC ISO）。
    AutoBackupLastBackupAt,
    /// 下次备份到期时间（Option<String>，UTC ISO）。
    AutoBackupNextDueAt,
    /// 后端日志等级（闭集五档 error/warn/info/debug/trace 的档位字符串，默认 info，
    /// 见 [`crate::logger::LogLevel`]）：后端消费、随 Backup/Restore 迁移（ADR-0006 / #611）。
    /// 持久化表示取档位指令字符串（同 [`crate::logger::LogLevel::directive`]）。
    LogLevel,
}

impl SettingKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutoBackupEnabled => "auto_backup.enabled",
            Self::AutoBackupDirty => "auto_backup.dirty",
            Self::AutoBackupLastBackupAt => "auto_backup.last_backup_at",
            Self::AutoBackupNextDueAt => "auto_backup.next_due_at",
            Self::LogLevel => "logging.level",
        }
    }
}

/// 读取配置值。key 缺失、甚至整表缺失（如恢复了 V008 之前的旧版本备份）
/// 时一律返回调用方声明的 `default`，行为免费正确；其它数据库错误照常上抛。
pub fn get<T: DeserializeOwned>(
    conn: &Connection,
    key: SettingKey,
    default: T,
) -> crate::error::Result<T> {
    let row: std::result::Result<String, _> = conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        [key.as_str()],
        |row| row.get(0),
    );
    match row {
        Ok(json) => serde_json::from_str(&json).map_err(crate::error::AppError::from),
        // 键不存在 → 默认值。
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default),
        // 表不存在（旧版本备份恢复后尚未迁移）→ 默认值。SQLite 对「缺表」
        // 只返回主码 SQLITE_ERROR（无专用扩展码），只能按消息文案匹配。
        Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("no such table") => {
            tracing::warn!(key = %key.as_str(), "app_settings 表不存在，返回默认值（旧版本备份？）");
            Ok(default)
        }
        Err(e) => Err(e.into()),
    }
}

/// 写入（upsert）配置值。value 为 serde_json 序列化结果。
/// 表不存在时（旧版本备份恢复后建表迁移被序列重排跳过等）就地建表后重试一次。
pub fn set<T: Serialize>(
    conn: &Connection,
    key: SettingKey,
    value: &T,
) -> crate::error::Result<()> {
    let json = serde_json::to_string(value)?;
    match conn.execute(
        "INSERT INTO app_settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key.as_str(), json],
    ) {
        Ok(_) => Ok(()),
        // 表不存在 → 就地建表（幂等）后重试一次，与 get 的缺表兑底对齐。
        Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("no such table") => {
            tracing::warn!(key = %key.as_str(), "app_settings 表不存在，就地创建后重试写入");
            conn.execute_batch(APP_SETTINGS_SQL)?;
            conn.execute(
                "INSERT INTO app_settings(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key.as_str(), json],
            )?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
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

    /// 迁移可重复执行幂等（CREATE TABLE IF NOT EXISTS）。
    #[test]
    fn migration_is_idempotent() {
        let mut c = db::open_in_memory().expect("打开内存库");
        db::init_db(&mut c).expect("首次迁移");
        c.execute_batch(APP_SETTINGS_SQL)
            .expect("重复执行同一建表语句");
    }

    /// 表缺失时写入自愈：就地建表后写入成功，随后可读回。
    #[test]
    fn set_creates_table_when_missing() {
        let c = db::open_in_memory().expect("未迁移的内存库");
        set(&c, SettingKey::AutoBackupDirty, &true).expect("缺表写入应自愈");
        let dirty: bool = get(&c, SettingKey::AutoBackupDirty, false).expect("读回");
        assert!(dirty);
    }

    /// 表尚未创建（旧版本备份缺表）时 get 返回默认值而非报错。
    #[test]
    fn get_returns_default_when_table_missing() {
        let c = db::open_in_memory().expect("未迁移的内存库");
        let v: bool = get(&c, SettingKey::AutoBackupEnabled, true).expect("缺表取默认");
        assert!(v);
    }

    /// key 缺失时返回调用方声明的默认值。
    #[test]
    fn get_returns_default_when_key_missing() {
        let c = conn();
        let v: Option<String> =
            get(&c, SettingKey::AutoBackupLastBackupAt, None).expect("读缺失键");
        assert_eq!(v, None);
    }

    /// 写入后读回与写入值一致（bool / 字符串多类型）。
    #[test]
    fn set_then_get_roundtrip() {
        let c = conn();
        set(&c, SettingKey::AutoBackupEnabled, &false).expect("写 enabled");
        let enabled: bool = get(&c, SettingKey::AutoBackupEnabled, true).expect("读 enabled");
        assert!(!enabled);

        let now = String::from("2026-02-17T08:00:00Z");
        set(&c, SettingKey::AutoBackupLastBackupAt, &Some(now.clone())).expect("写 last_backup_at");
        let at: Option<String> =
            get(&c, SettingKey::AutoBackupLastBackupAt, None).expect("读 last_backup_at");
        assert_eq!(at, Some(now));

        // 覆盖写：同 key 二次写入以新值为准。
        set(&c, SettingKey::AutoBackupDirty, &true).expect("覆写 dirty");
        set(&c, SettingKey::AutoBackupDirty, &false).expect("覆写 dirty");
        let dirty: bool = get(&c, SettingKey::AutoBackupDirty, true).expect("读 dirty");
        assert!(!dirty);
    }

    /// 枚举外的裸字符串 key 无法经编译路径构造：key 只能来自枚举变体，
    /// 这里校验各变体映射到规范 `<feature>.<name>` 字符串。
    #[test]
    fn keys_are_feature_dot_name() {
        assert_eq!(
            SettingKey::AutoBackupEnabled.as_str(),
            "auto_backup.enabled"
        );
        assert_eq!(SettingKey::AutoBackupDirty.as_str(), "auto_backup.dirty");
        assert_eq!(
            SettingKey::AutoBackupLastBackupAt.as_str(),
            "auto_backup.last_backup_at"
        );
        assert_eq!(
            SettingKey::AutoBackupNextDueAt.as_str(),
            "auto_backup.next_due_at"
        );
        assert_eq!(SettingKey::LogLevel.as_str(), "logging.level");
    }
}
