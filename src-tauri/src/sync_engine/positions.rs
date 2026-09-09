//! 位点（StreamPosition，issue #857 / ADR-0091 决策 9）：本机对各来源设备 op 流
//! 的「已应用位点」——`sync_stream_positions` 表的唯一 SQL 收口。
//!
//! 位点语义：`applied_through` 是该流上已裁决 op 的**连续前缀水位**——位点之前
//! 的 op 已全部并入本端状态（应用/去重/按位点门跳过）或作为 LWW 输者落日志可
//! 追溯（压制）。位点**永不越过未裁决的 op**：挂起中的 op 不进日志（未应用），
//! 位点停在它之前（安全钉住），增量拉取仍会拿到它，重投递自然重试——这是
//! 「截断不丢失未应用 op」的机制根据。
//!
//! 推进规则（[`advance`]，仅在 op 裁决落定后调用）：
//! - 流首见：以该 op 时钟建行（本端从此刻开始跟踪该流）；
//! - 已有行且裁决时钟更大：从水位起就近日志**连续前滚**——吸收挂起出队后的
//!   补齐（水位跨过已补齐区段），遇缺口（仍有未应用 op）即停；
//! - 其余情形水位已覆盖，不动。
//!
//! 位点在截断后依然留存（水位不因日志缩短而回退）；本机产出的 op 同样推进
//! 自己流的位点——源端截掉旧 op 后对端全量重投，位点门拦住已截掉的 op 不复活。

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::now_iso;
use crate::error::Result;

/// 单个来源设备 op 流的已应用位点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPosition {
    /// 来源设备标识（DeviceId）。
    pub device_id: String,
    /// 该流已应用到的时钟（连续前缀水位；此之前的 op 全部并入本端）。
    pub applied_through: i64,
}

/// 位点清单（按 DeviceId 序稳定返回）：Checkpoint 位点组件与通道 manifest 上报
/// 位点的数据面。
pub(super) fn list(conn: &Connection) -> Result<Vec<StreamPosition>> {
    let mut stmt = conn.prepare(
        "SELECT device_id, applied_through FROM sync_stream_positions ORDER BY device_id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(StreamPosition {
            device_id: r.get(0)?,
            applied_through: r.get(1)?,
        })
    })?;
    let mut positions = Vec::new();
    for row in rows {
        positions.push(row?);
    }
    Ok(positions)
}

/// 单流位点（无行返回 None——该流尚未跟踪，位点门不生效）。
pub(super) fn position_of(conn: &Connection, device_id: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT applied_through FROM sync_stream_positions WHERE device_id = ?1",
            params![device_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// 位点推进（op 裁决落定后调用；推进规则见模块文档）。
pub(super) fn advance(conn: &Connection, device_id: &str, decided_clock: i64) -> Result<()> {
    match position_of(conn, device_id)? {
        None => {
            conn.execute(
                "INSERT INTO sync_stream_positions (device_id, applied_through, updated_at) \
                 VALUES (?1, ?2, ?3)",
                params![device_id, decided_clock, now_iso()],
            )?;
            Ok(())
        }
        Some(position) if decided_clock > position => {
            // 连续前滚：水位 +1 已在日志则前移，吸收挂起补齐后的区段；遇缺口即停
            //（缺口 op 未应用，水位不越过——安全钉住）。
            let mut through = position;
            while super::ops::is_known_at(conn, device_id, through + 1)? {
                through += 1;
            }
            if through > position {
                conn.execute(
                    "UPDATE sync_stream_positions SET applied_through = ?2, updated_at = ?3 \
                     WHERE device_id = ?1",
                    params![device_id, through, now_iso()],
                )?;
            }
            Ok(())
        }
        Some(_) => Ok(()),
    }
}

/// 位点整体覆写（Checkpoint 引导专用）：以 Checkpoint 携带的位点为准重建位点表
/// （快照时刻各流头）。引导目标为空库时等价于初建。
pub(super) fn replace_all(conn: &Connection, positions: &[StreamPosition]) -> Result<()> {
    conn.execute("DELETE FROM sync_stream_positions", [])?;
    for position in positions {
        conn.execute(
            "INSERT INTO sync_stream_positions (device_id, applied_through, updated_at) \
             VALUES (?1, ?2, ?3)",
            params![position.device_id, position.applied_through, now_iso()],
        )?;
    }
    Ok(())
}
