use rusqlite::Connection;
use serde::Serialize;
use serde::de::DeserializeOwned;

const APP_SETTINGS_SQL: &str = include_str!("../../../migrations/V008__app_settings.sql");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
    AutoBackupEnabled,
    AutoBackupDirty,
    AutoBackupLastBackupAt,
    AutoBackupConsecutiveFailures,
    AutoBackupNextDueAt,
    LogLevel,
    LedgerBaseCurrency,
    SyncChannelConfig,
    SyncLastSyncAt,
    BookFingerprint,
}

impl SettingKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutoBackupEnabled => "auto_backup.enabled",
            Self::AutoBackupDirty => "auto_backup.dirty",
            Self::AutoBackupLastBackupAt => "auto_backup.last_backup_at",
            Self::AutoBackupConsecutiveFailures => "auto_backup.consecutive_failures",
            Self::AutoBackupNextDueAt => "auto_backup.next_due_at",
            Self::LogLevel => "logging.level",
            Self::LedgerBaseCurrency => "ledger.base_currency",
            Self::SyncChannelConfig => "sync.channel.config",
            Self::SyncLastSyncAt => "sync.last_sync_at",
            Self::BookFingerprint => "book.fingerprint",
        }
    }
}

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
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default),
        Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("no such table") => {
            tracing::warn!(key = %key.as_str(), "app_settings 表不存在，返回默认值（旧版本备份？）");
            Ok(default)
        }
        Err(e) => Err(e.into()),
    }
}

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

pub fn clear(conn: &Connection, key: SettingKey) -> crate::error::Result<()> {
    match conn.execute("DELETE FROM app_settings WHERE key = ?1", [key.as_str()]) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("no such table") => {
            tracing::warn!(key = %key.as_str(), "app_settings 表不存在，清除按无操作处理");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> rusqlite::Connection {
        tauri_app_lib::test_support::open()
    }

    #[test]
    fn migration_is_idempotent() {
        let c = tauri_app_lib::test_support::open();
        c.execute_batch(APP_SETTINGS_SQL)
            .expect("重复执行同一建表语句");
    }

    fn conn_without_app_settings() -> rusqlite::Connection {
        let c = tauri_app_lib::test_support::open();
        c.execute("DROP TABLE app_settings", [])
            .expect("删除 app_settings");
        c
    }

    #[test]
    fn set_creates_table_when_missing() {
        let c = conn_without_app_settings();
        set(&c, SettingKey::AutoBackupDirty, &true).expect("缺表写入应自愈");
        let dirty: bool = get(&c, SettingKey::AutoBackupDirty, false).expect("读回");
        assert!(dirty);
    }

    #[test]
    fn get_returns_default_when_table_missing() {
        let c = conn_without_app_settings();
        let v: bool = get(&c, SettingKey::AutoBackupEnabled, true).expect("缺表取默认");
        assert!(v);
    }

    #[test]
    fn get_returns_default_when_key_missing() {
        let c = conn();
        let v: Option<String> =
            get(&c, SettingKey::AutoBackupLastBackupAt, None).expect("读缺失键");
        assert_eq!(v, None);
    }

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

        set(&c, SettingKey::AutoBackupDirty, &true).expect("覆写 dirty");
        set(&c, SettingKey::AutoBackupDirty, &false).expect("覆写 dirty");
        let dirty: bool = get(&c, SettingKey::AutoBackupDirty, true).expect("读 dirty");
        assert!(!dirty);
    }

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
        assert_eq!(SettingKey::BookFingerprint.as_str(), "book.fingerprint");
    }
}
