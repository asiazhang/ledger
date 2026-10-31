//! 账本身份（ADR-0144 决策 1/5，issue #1984）：账本指纹与产物来源设备标识。
//!
//! - **账本指纹**（词汇表：参考数据与设置域「账本指纹（Book Fingerprint）」）：
//!   账本跳设备、跳目录的稳定身份，随数据走而不随路径走。内容锚取库内最早
//!   一笔交易的全局唯一标识（排序口径与交易列表行序同款：
//!   `date ASC, created_at ASC, id ASC`，排除软删——跨端恢复或同步后必然
//!   相同），首用算出即固化进 AppSettings（[`SettingKey::BookFingerprint`]，
//!   零 schema 迁移），此后恒读固化值；空库（尚无交易）退化为端内生成随机
//!   标识（同款「首用生成并持久化」语义见多端同步域 DeviceId）。同一逻辑
//!   账本在任意设备、任意目录下指纹相同——句柄与路径不参与计算。
//! - **来源设备标识**：多端同步域 DeviceId（`ledger_sync_protocol::device`，
//!   安装实例级，重装即新设备，ADR-0091）。取不到（attached 侧异常）时该轮
//!   产物不带设备水印 = 来源未知，不阻断备份（ADR-0144 决策 5）。
//!
//! 两者都是备份产物的自述水印（内层元数据明文 `fingerprint` / `device_id`，
//! 只含标识不含密钥）；固化的设置写入走 AppSettings 置脏豁免通道（ADR-0032：
//! 设置写入不算「账本数据变化」，不该触发待备份置脏）。

use rusqlite::{Connection, OptionalExtension};

use ledger_infra::db::new_uuid;
use ledger_infra::error::Result;
use ledger_infra::settings::{self, SettingKey};

/// 库内最早一笔交易的全局唯一标识（内容锚，ADR-0144 决策 1）：排序口径与
/// 交易列表行序同款（`date ASC, created_at ASC, id ASC`，id 最终 tiebreaker），
/// 排除软删行；空库返回 `None`。
fn content_anchor(conn: &Connection) -> Result<Option<String>> {
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

/// 当前账本指纹（读侧唯一入口）：已固化恒读固化值；未固化则按内容锚计算
///（空库退化为端内生成 UUIDv7）并固化，算出即恒定。调用方持连接锁（备份
/// 产出段在锁内），读后写无竞态。
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

/// 产物来源设备标识：多端同步域 DeviceId（首用生成并持久化，与同步写路径
/// 同款语义）；获取失败（attached 侧异常）返回 `None`——该轮产物不带设备
/// 水印（来源未知），不阻断备份（ADR-0144 决策 5）。
pub fn source_device_id(conn: &Connection) -> Option<String> {
    match ledger_sync_protocol::device::device_id(conn) {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!(error = %e, "设备标识不可得，本轮产物将不带来源设备水印");
            None
        }
    }
}
