use rusqlite::{Connection, OptionalExtension};

use ledger_infra::db::new_uuid;
use ledger_infra::error::Result;
use ledger_infra::settings::{self, SettingKey};

pub(crate) fn content_anchor(conn: &Connection) -> Result<Option<String>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM transactions WHERE is_deleted = 0 \
             ORDER BY date ASC, created_at ASC, id ASC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id)
}

pub fn book_fingerprint(conn: &Connection) -> Result<String> {
    let fixed: Option<String> = settings::get(conn, SettingKey::BookFingerprint, None)?;
    if let Some(fp) = fixed {
        return Ok(fp);
    }
    let fp = content_anchor(conn)?.unwrap_or_else(new_uuid);
    settings::set(conn, SettingKey::BookFingerprint, &fp)?;
    tracing::debug!(fingerprint = %fp, "账本指纹首用固化");
    Ok(fp)
}

pub fn book_fingerprint_readonly(conn: &Connection) -> Result<Option<String>> {
    let fixed: Option<String> = settings::get(conn, SettingKey::BookFingerprint, None)?;
    if fixed.is_some() {
        return Ok(fixed);
    }
    content_anchor(conn)
}

pub fn source_device_id(conn: &Connection) -> Option<String> {
    match ledger_sync_protocol::device::device_id(conn) {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!(error = %e, "设备标识不可得，本轮产物将不带来源设备水印");
            None
        }
    }
}
