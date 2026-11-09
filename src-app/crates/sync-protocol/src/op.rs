//! op 行落库与读取：`sync_ops` 表的唯一 SQL 收口（自 sync_engine/ops.rs 下放，
//! issue #1089）。
//!
//! 本地 op 产出（[`record_local`]，业务域命令模块经泛型契约调用）与裸部件落库
//! （[`insert_row`]，重放事务内由同步引擎调用）共用同一行形态；本地路径的信封
//! 组装（实体标签 + serde 载荷 → `{"entity","payload"}` JSON）收口在此，与
//! sync_engine 的 `DomainCommand` serde 信封同形（tag/content 同名同序），读写
//! 往返同源。域 payload 类型不进本 crate——协议面只认 [`command::SyncCommand`]
//! 的形状。
//!
//! 写后钩子（ADR-0091 决策 9）挂在 [`record_local`]——本地产出 op 的「起来看看」
//! 提示。信号**登记点反转**（#1089）：本 crate 是 rank 0 底座，不认识同步调度；
//! 调度侧在启动时装入响应闭包（[`install_after_write_hook`]），钩子未装时通知
//! 零动作——写路径对同步域完全无感。

use std::sync::OnceLock;

use rusqlite::{Connection, OptionalExtension, params};

use ledger_infra::db::{new_uuid, now_iso, schema_version};
use ledger_infra::error::{AppError, Result};

use super::command::SyncCommand;
use super::device;
use super::position;

/// 写后钩子的进程级单例（登记点反转的承接面）：op 产出单点经它投递一次「有新
/// op 待发布」。先装者优先、重复安装零动作；未装（调度未拉起 / 单测环境）时
/// 通知是零动作——与迁移前 sync_engine 的 `OnceLock<Sender<()>>` 信号槽同纪律，
/// 只是「信号是什么」的知识上收给安装方（协议面不持有通道类型）。
static AFTER_WRITE_HOOK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// 安装写后钩子（登记点，#1089）：同步调度在启动时装入响应闭包（去抖合流跑
/// 一轮）。重复安装零动作并返回 `false`（先装者优先，幂等）。
pub fn install_after_write_hook(hook: impl Fn() + Send + Sync + 'static) -> bool {
    AFTER_WRITE_HOOK.set(Box::new(hook)).is_ok()
}

/// 写后通知（信号侧单点，[`record_local`] 内部消费）：钩子未装时零动作。
fn notify_after_write() {
    if let Some(hook) = AFTER_WRITE_HOOK.get() {
        hook();
    }
}

/// 本地 op 产出的簿记事实（不含载荷）：调用方（sync_engine 适配层）与自身持有
/// 的命令组装完整 op。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedOp {
    /// op 标识（UUID v7，产出端生成；幂等去重键）。
    pub op_id: String,
    /// 来源设备标识（DeviceId）。
    pub device_id: String,
    /// 来源端内单调逻辑时钟。
    pub clock: i64,
    /// 产生时 schema 版本（SQLite `user_version`）。
    pub schema_version: i64,
}

/// op 行的裸部件（[`insert_row`] 与裸读取面的行形态）：载荷已是信封 JSON 文本。
///
/// 裸部件面服务于重放路径——外来 op 的信封已由 sync_engine 持有（serde 域信封
/// 序列化），协议面只负责行形态与簿记，不重复载荷知识。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpRow {
    /// op 标识（幂等去重键）。
    pub op_id: String,
    /// 来源设备标识。
    pub device_id: String,
    /// 来源端内单调逻辑时钟。
    pub clock: i64,
    /// 产生时 schema 版本。
    pub schema_version: i64,
    /// 实体标签（信封 tag 与 `entity` 列同源）。
    pub entity: String,
    /// 实体键（无实体指向的命令为空串，与列默认同形）。
    pub entity_id: String,
    /// 信封 JSON 文本（`{"entity","payload"}` 形态）。
    pub payload: String,
}

/// 裸 op 行（[`read_all_raw`] / [`read_own_since_raw`] 的行形态）：载荷保持信封
/// JSON 文本，反序列化（→ 域信封）由调用方承担——协议面不认识域命令类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawOp {
    /// op 标识。
    pub op_id: String,
    /// 来源设备标识。
    pub device_id: String,
    /// 来源端内单调逻辑时钟。
    pub clock: i64,
    /// 产生时 schema 版本。
    pub schema_version: i64,
    /// 信封 JSON 文本。
    pub payload: String,
}

/// 本地 op 产出（op 产出信封单点）：分配 DeviceId / 逻辑时钟 / schema 版本，
/// 经 [`SyncCommand`] 契约组装信封并落日志，推进本机流位点，投递写后通知。
///
/// 必须在本地写事务内调用（行为编排入口保证）——op 与其对应的数据写同事务
/// 提交/回滚，写失败不残留 op，时钟不产生空洞。
///
/// 信封组装（与 `DomainCommand` serde 信封同形）：`{"entity": ENTITY,
/// "payload": <命令 serde 形态>}`——字段同名同序，载荷反序列化（域信封枚举）
/// 与迁移前逐字节兼容（只增不改契约）。
///
/// 写后通知在事务内投递：它只是「起来看看」的提示，真正的读在调度线程拿到
/// 连接锁之后（写路径持锁到提交/回滚，轮次看不到未提交状态）；非阻塞，钩子
/// 未装时零动作。
pub fn record_local<C: SyncCommand>(conn: &Connection, command: &C) -> Result<RecordedOp> {
    let device_id = device::device_id(conn)?;
    let clock = device::next_clock(conn)?;
    let op_id = new_uuid();
    let schema_version = schema_version(conn)?;
    // 信封组装（serde 结构体单点）：标签取契约 ENTITY，载荷取命令 serde 形态。
    #[derive(serde::Serialize)]
    struct EnvelopeRef<'a> {
        entity: &'a str,
        payload: serde_json::Value,
    }
    let payload = serde_json::to_value(command)
        .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?;
    let envelope = serde_json::to_string(&EnvelopeRef {
        entity: C::ENTITY,
        payload,
    })
    .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?;
    // 实体键由契约派生（标签恒有、键可空，ADR-0101 决策 2）：`entity` 列取标签，
    // `entity_id` 取实体键（无实体指向的命令为空串）。
    let row = OpRow {
        op_id: op_id.clone(),
        device_id: device_id.clone(),
        clock,
        schema_version,
        entity: C::ENTITY.to_string(),
        entity_id: command
            .subject()
            .map(|id| id.into_owned())
            .unwrap_or_default(),
        payload: envelope,
    };
    insert_row(conn, &row)?;
    // 本机流位点同步推进（同一写事务内）：本端产出的 op 即刻裁决落定，水位
    // 跟进使位点门能拦住「源端截掉旧 op 后对端全量重投」的本机旧 op（不复活，
    // issue #857）。
    position::advance(conn, &row.device_id, clock)?;
    // 写后即时入队上传（ADR-0091 决策 9）：钩子由同步调度启动时装入；未装
    // （单测 / 未拉起调度）时零动作。
    notify_after_write();
    Ok(RecordedOp {
        op_id,
        device_id,
        clock,
        schema_version,
    })
}

/// op 行落库（裸部件面，重放路径：重放事务内调用，与命令执行同事务原子）。
pub fn insert_row(conn: &Connection, row: &OpRow) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_ops (op_id, device_id, clock, schema_version, entity, entity_id, payload, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            row.op_id,
            row.device_id,
            row.clock,
            row.schema_version,
            row.entity,
            row.entity_id,
            row.payload,
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
pub fn has_later_subject(
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
pub fn is_known(conn: &Connection, op_id: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sync_ops WHERE op_id = ?1",
            params![op_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// 流内某时钟的 op 是否已知（位点连续前滚的日志在位判定；(device_id, clock)
/// 唯一索引支撑）。
pub fn is_known_at(conn: &Connection, device_id: &str, clock: i64) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sync_ops WHERE device_id = ?1 AND clock = ?2",
            params![device_id, clock],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// 截断单流日志前缀（时钟 ≤ `through_position`）：仅删 `sync_ops` 行，返回删
/// 除行数。挂起队列不进本表（未应用），天然不受影响；位点表独立留存、水位不
/// 回退。调用界（owner 门与水位来源）在 sync_engine 的 Checkpoint 截断入口。
pub fn delete_stream_before(
    conn: &Connection,
    device_id: &str,
    through_position: i64,
) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM sync_ops WHERE device_id = ?1 AND clock <= ?2",
        params![device_id, through_position],
    )?;
    Ok(deleted)
}

/// 日志是否为空（引导守卫用：目标已有日志即已参与同步）。
pub fn is_empty(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sync_ops", [], |r| r.get(0))?;
    Ok(count == 0)
}

/// 读取全部 op 行的裸形态（本地产出 + 已重放的外来 op）。不排序：排序知识
/// 单点在 sync_engine 的全序入口（调用方消费）。
pub fn read_all_raw(conn: &Connection) -> Result<Vec<RawOp>> {
    let mut stmt =
        conn.prepare("SELECT op_id, device_id, clock, schema_version, payload FROM sync_ops")?;
    let rows = stmt.query_map([], map_op_row)?;
    let mut ops = Vec::new();
    for row in rows {
        ops.push(row?);
    }
    Ok(ops)
}

/// 读取本机产出且时钟晚于 `after_clock` 的 op 裸形态（通道上传接缝：只发布
/// 自己流，按时钟序返回；通道上传位点以 manifest 为权威）。
pub fn read_own_since_raw(
    conn: &Connection,
    device_id: &str,
    after_clock: i64,
) -> Result<Vec<RawOp>> {
    let mut stmt = conn.prepare(
        "SELECT op_id, device_id, clock, schema_version, payload FROM sync_ops \
         WHERE device_id = ?1 AND clock > ?2 ORDER BY clock ASC",
    )?;
    let rows = stmt.query_map(params![device_id, after_clock], map_op_row)?;
    let mut ops = Vec::new();
    for row in rows {
        ops.push(row?);
    }
    Ok(ops)
}

/// op 行读取映射（读面共用）。
fn map_op_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawOp> {
    Ok(RawOp {
        op_id: r.get(0)?,
        device_id: r.get(1)?,
        clock: r.get(2)?,
        schema_version: r.get(3)?,
        payload: r.get(4)?,
    })
}
