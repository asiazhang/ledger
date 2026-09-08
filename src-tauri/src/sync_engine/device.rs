//! 设备标识与逻辑时钟：同步元数据的本机单点（`sync_device` 单行）。
//!
//! DeviceId 首用生成（UUID v7）并持久化；重装/换机 = 新库 = 新标识，属合法
//! 路径（ADR-0091）。逻辑时钟端内严格递增，分配发生在本地写事务内——随事务
//! 提交/回滚，为跨端全序 (clock, device_id) 提供来源侧排序键。
//!
//! 调用约定：[`device_id`] 只在本地写路径消费（审计列身份 + op 产出）；全部
//! 写路径都处于事务中（ADR-0033 接缝契约），首次调用由此把 `sync_device` 行
//! 带入当前事务一并持久化。

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::{new_uuid, now_iso};
use crate::error::Result;

/// 当前设备标识（DeviceId）：`sync_device` 单行的 id。
///
/// 无行则生成 UUID v7 并持久化（首用生成语义）；此后恒读持久化值。单连接
/// 互斥（[`crate::db::DbState`]）串行化调用，读后写无竞态。
pub(crate) fn device_id(conn: &Connection) -> Result<String> {
    let existing: Option<String> = conn
        .query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO sync_device (id, logical_clock, created_at, updated_at) VALUES (?1, 0, ?2, ?2)",
        params![id, now],
    )?;
    tracing::debug!(device_id = %id, "首次使用：已生成本机设备标识");
    Ok(id)
}

/// 分配下一个端内逻辑时钟（+1 并持久化）。
///
/// 必须在本地写事务内调用（[`super::ops::record_local`] 保证：先取 DeviceId
/// 再分配时钟，均在 op 产出的同一事务中，随事务提交/回滚）。
pub(crate) fn next_clock(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "UPDATE sync_device SET logical_clock = logical_clock + 1, updated_at = ?1 \
         RETURNING logical_clock",
        params![now_iso()],
        |r| r.get(0),
    )
    .map_err(Into::into)
}
