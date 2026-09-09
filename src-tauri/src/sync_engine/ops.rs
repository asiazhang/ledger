//! op 行落库与读取：`sync_ops` 表的唯一 SQL 收口。
//!
//! 本地 op 产出（[`record_local`]，行为编排入口经各域命令模块调用）与外来 op
//! 落日志（[`insert_row`]，重放事务内由同步引擎调用）共用同一行形态；载荷
//! 序列化（DomainCommand → JSON）收口在此，读写往返同源。

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::{new_uuid, now_iso};
use crate::error::{AppError, Result};

use super::command::DomainCommand;
use super::device;
use super::model::SyncOp;

/// 本地 op 产出（op 产出信封单点）：分配 DeviceId / 逻辑时钟 / schema 版本，
/// 序列化载荷并落日志，返回完整 op。
///
/// 必须在本地写事务内调用（行为编排入口保证）——op 与其对应的数据写同事务
/// 提交/回滚，写失败不残留 op，时钟不产生空洞。
pub(crate) fn record_local(conn: &Connection, command: DomainCommand) -> Result<SyncOp> {
    let device_id = device::device_id(conn)?;
    let clock = device::next_clock(conn)?;
    let op = SyncOp {
        op_id: new_uuid(),
        device_id,
        clock,
        schema_version: crate::db::schema_version(conn)?,
        command,
    };
    insert_row(conn, &op)?;
    Ok(op)
}

/// 外来 op 落日志（重放事务内调用，与命令执行同事务原子）。
pub(crate) fn insert_row(conn: &Connection, op: &SyncOp) -> Result<()> {
    // 序列化失败属程序缺陷（载荷为本仓自有类型）：非码化 Invalid、fail loud；
    // 「旧端载荷反序列化失败」的 schema 偏斜场景由挂起队列承接后改道。
    let payload = serde_json::to_string(&op.command)
        .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?;
    conn.execute(
        "INSERT INTO sync_ops (op_id, device_id, clock, schema_version, entity, entity_id, payload, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            op.op_id,
            op.device_id,
            op.clock,
            op.schema_version,
            op.command.entity(),
            op.command
                .subject()
                .map(|(_, id)| id)
                .unwrap_or_default(),
            payload,
            now_iso(),
        ],
    )?;
    Ok(())
}

/// LWW 裁决检索：本地日志中是否存在同实体且全序更后的 op（ADR-0091 决策 4）。
///
/// 全序更后 = (clock, device_id) 字典序更大；命中即意味着序末者已在本端生效，
/// 全序更前的同实体 op 为输者（落日志可追溯、不执行）。无实体指向的调用方
/// （冲突域在 OccurrenceKey 的命令）不进本查询。
pub(crate) fn has_later_subject(
    conn: &Connection,
    entity: &str,
    entity_id: &str,
    clock: i64,
    device_id: &str,
) -> Result<bool> {
    let later = conn
        .query_row(
            "SELECT 1 FROM sync_ops \
             WHERE entity = ?1 AND entity_id = ?2 \
               AND (clock > ?3 OR (clock = ?3 AND device_id > ?4)) \
             LIMIT 1",
            params![entity, entity_id, clock, device_id],
            |_| Ok(()),
        )
        .optional()?;
    Ok(later.is_some())
}

/// op 是否已知（幂等判定单点）：按 `op_id` 主键存在性。
pub(crate) fn is_known(conn: &Connection, op_id: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sync_ops WHERE op_id = ?1",
            params![op_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// 读取全部 op 行（本地产出 + 已重放的外来 op；序列化与 [`insert_row`] 同源）。
/// 不排序：排序知识单点在 [`super::engine::total_order`]（read_ops 调用方消费）。
pub(crate) fn read_all(conn: &Connection) -> Result<Vec<SyncOp>> {
    let mut stmt =
        conn.prepare("SELECT op_id, device_id, clock, schema_version, payload FROM sync_ops")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut ops = Vec::new();
    for row in rows {
        let (op_id, device_id, clock, schema_version, payload) = row?;
        // 反序列化失败：本仓自有类型恒成功（wire 接入路径的旧载荷不可解析场景
        // 由 [`super::engine::ingest_ops`] 挂起承接，不进本函数）。
        let command = serde_json::from_str(&payload)
            .map_err(|e| AppError::Invalid(format!("op 载荷反序列化失败: {e}")))?;
        ops.push(SyncOp {
            op_id,
            device_id,
            clock,
            schema_version,
            command,
        });
    }
    Ok(ops)
}
