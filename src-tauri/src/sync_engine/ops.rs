//! op 行落库与读取的**适配层**（#1089）：`sync_ops` 表的唯一 SQL 收口已下放
//! 协议 crate（[`ledger_sync_protocol::op`]）；本模块只保留 sync_engine 内部的
//! 签名形态，承载域信封的**载荷知识**——`DomainCommand` ↔ JSON 的序列化/反序
//! 列化；行形态与簿记（DeviceId / 时钟 / schema 版本 / 位点推进）归协议面。
//!
//! - 本地 op 产出：业务域经命令模块直呼协议面
//!   [`ledger_sync_protocol::op::record_local`]（泛型于命令契约，域命令类型
//!   自述实体标签与实体键），不再经本模块——sync_engine 与业务域的最后一处
//!   互相依赖由此断开（issue #1089）；
//! - 外来 op 落日志（[`insert_row`]，重放事务内由引擎调用）与读取
//!   （[`read_all`] / [`read_own_since`]）：载荷在本层与域信封互转，读写往返
//!   同源。

use rusqlite::Connection;

use crate::error::{AppError, Result};

use super::model::SyncOp;

/// 外来 op 落日志（重放事务内调用，与命令执行同事务原子）：域信封序列化后
/// 经协议面裸部件落库（行形态与簿记单点在协议 crate）。
pub(crate) fn insert_row(conn: &Connection, op: &SyncOp) -> Result<()> {
    // 序列化失败属程序缺陷（载荷为本仓自有类型）：非码化 Invalid、fail loud；
    // 「旧端载荷反序列化失败」的 schema 偏斜场景由挂起队列承接后改道。
    let payload = serde_json::to_string(&op.command)
        .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?;
    // 标签恒有、键可空（ADR-0101 决策 2）：列取值读 `subject()`——`entity` 列取
    // 标签（与 serde tag 同源），`entity_id` 取实体键（无实体指向的命令为空串）。
    let (entity, entity_id) = op.command.subject();
    ledger_sync_protocol::op::insert_row(
        conn,
        &ledger_sync_protocol::op::OpRow {
            op_id: op.op_id.clone(),
            device_id: op.device_id.clone(),
            clock: op.clock,
            schema_version: op.schema_version,
            entity: entity.to_string(),
            entity_id: entity_id.map(|id| id.into_owned()).unwrap_or_default(),
            payload,
        },
    )
}

/// LWW 裁决检索：本地日志中是否存在同实体且全序更后的 op（ADR-0091 决策 4）。
///
/// 全序更后 = (clock, device_id) 字典序更大；命中即意味着序末者已在本端生效，
/// 全序更前的同实体 op 为输者（落日志可追溯、不执行）。
pub(crate) fn has_later_subject(
    conn: &Connection,
    entity: &str,
    entity_id: &str,
    clock: i64,
    device_id: &str,
) -> Result<bool> {
    ledger_sync_protocol::op::has_later_subject(conn, entity, entity_id, clock, device_id)
}

/// op 是否已知（幂等判定单点）：按 `op_id` 主键存在性。
pub(crate) fn is_known(conn: &Connection, op_id: &str) -> Result<bool> {
    ledger_sync_protocol::op::is_known(conn, op_id)
}

/// 截断单流日志前缀（时钟 ≤ `through_position`）：仅删 `sync_ops` 行，返回删
/// 除行数。调用界（owner 门与水位来源）在 [`super::checkpoint::truncate_stream_before`]。
pub(crate) fn delete_stream_before(
    conn: &Connection,
    device_id: &str,
    through_position: i64,
) -> Result<usize> {
    ledger_sync_protocol::op::delete_stream_before(conn, device_id, through_position)
}

/// 日志是否为空（引导守卫用：目标已有日志即已参与同步）。
pub(crate) fn is_empty(conn: &Connection) -> Result<bool> {
    ledger_sync_protocol::op::is_empty(conn)
}

/// 读取全部 op 行（本地产出 + 已重放的外来 op；载荷反序列化与落库同源）。
/// 不排序：排序知识单点在 [`super::engine::total_order`]（read_ops 调用方消费）。
pub(crate) fn read_all(conn: &Connection) -> Result<Vec<SyncOp>> {
    let mut ops = Vec::new();
    for raw in ledger_sync_protocol::op::read_all_raw(conn)? {
        ops.push(op_from_raw(raw)?);
    }
    Ok(ops)
}

/// 读取本机产出且时钟晚于 `after_clock` 的 op（通道上传接缝：只发布自己流，
/// 按时钟序返回；通道上传位点以 manifest 为权威，见 `channel::publish_own_ops`）。
pub(crate) fn read_own_since(
    conn: &Connection,
    device_id: &str,
    after_clock: i64,
) -> Result<Vec<SyncOp>> {
    let mut ops = Vec::new();
    for raw in ledger_sync_protocol::op::read_own_since_raw(conn, device_id, after_clock)? {
        ops.push(op_from_raw(raw)?);
    }
    Ok(ops)
}

/// 裸 op 行 → op 信封（载荷反序列化同源单点；反序列化失败属程序缺陷，fail
/// loud——wire 侧不可解析载荷由 [`super::engine::ingest_ops`] 挂起承接）。
fn op_from_raw(raw: ledger_sync_protocol::op::RawOp) -> Result<SyncOp> {
    let command = serde_json::from_str(&raw.payload)
        .map_err(|e| AppError::Invalid(format!("op 载荷反序列化失败: {e}")))?;
    Ok(SyncOp {
        op_id: raw.op_id,
        device_id: raw.device_id,
        clock: raw.clock,
        schema_version: raw.schema_version,
        command,
    })
}
