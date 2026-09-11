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
use super::positions;

/// 本地 op 产出（op 产出信封单点）：分配 DeviceId / 逻辑时钟 / schema 版本，
/// 序列化载荷并落日志，返回完整 op。
///
/// 必须在本地写事务内调用（行为编排入口保证）——op 与其对应的数据写同事务
/// 提交/回滚，写失败不残留 op，时钟不产生空洞。
///
/// 本函数是**本地产出 op 的唯一收敛点**（行为编排入口经各域命令模块调用；
/// 外来 op 重放走 [`insert_row`]、不产出本地 op，天然不误触发）：写后即时上传
/// 的信号（issue #863 / ADR-0091 决策 9）钩在这里——比壳层 `write_entry` 更准
/// （后者漏掉定时追补 `scheduled_transactions::run_catch_up` 并在 `sync_now`
/// 自身空触发），也比连接层 `db::write` 提交点更合分层（基础设施不持业务语义，
/// ADR-0071 决策 6）。信号在事务内投递：它只是「起来看看」的提示，真正的读在
/// 调度线程拿到连接锁之后（写路径持锁到提交/回滚，轮次看不到未提交状态）；
/// 非阻塞，调度未拉起时零动作。
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
    // 本机流位点同步推进（同一写事务内）：本端产出的 op 即刻裁决落定，水位
    // 跟进使位点门能拦住「源端截掉旧 op 后对端全量重投」的本机旧 op（不复活，
    // issue #857）。
    positions::advance(conn, &op.device_id, op.clock)?;
    // 写后即时入队上传（ADR-0091 决策 9）：本地产出 op 即投递一次「有新 op
    // 待发布」，调度线程去抖合流后跑一轮。非阻塞（无界通道投递即返回），
    // 写路径不等网络；同步调度未拉起（未配置通道 / 单测环境）时零动作。
    super::trigger::sync_after_write();
    Ok(op)
}

/// 外来 op 落日志（重放事务内调用，与命令执行同事务原子）。
pub(crate) fn insert_row(conn: &Connection, op: &SyncOp) -> Result<()> {
    // 序列化失败属程序缺陷（载荷为本仓自有类型）：非码化 Invalid、fail loud；
    // 「旧端载荷反序列化失败」的 schema 偏斜场景由挂起队列承接后改道。
    let payload = serde_json::to_string(&op.command)
        .map_err(|e| AppError::Invalid(format!("op 载荷序列化失败: {e}")))?;
    // 标签恒有、键可空（ADR-0101 决策 2）：列取值读 `subject()`——`entity` 列取
    // 标签（与 serde tag 同源），`entity_id` 取实体键（无实体指向的命令为空串）。
    let (entity, entity_id) = op.command.subject();
    conn.execute(
        "INSERT INTO sync_ops (op_id, device_id, clock, schema_version, entity, entity_id, payload, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            op.op_id,
            op.device_id,
            op.clock,
            op.schema_version,
            entity,
            entity_id.map(|id| id.into_owned()).unwrap_or_default(),
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

/// 流内某时钟的 op 是否已知（位点连续前滚的日志在位判定；(device_id, clock)
/// 唯一索引支撑）。
pub(super) fn is_known_at(conn: &Connection, device_id: &str, clock: i64) -> Result<bool> {
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
/// 回退。调用界（owner 门与水位来源）在 [`super::checkpoint::truncate_stream_before`]。
pub(super) fn delete_stream_before(
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
pub(super) fn is_empty(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sync_ops", [], |r| r.get(0))?;
    Ok(count == 0)
}

/// 读取全部 op 行（本地产出 + 已重放的外来 op；序列化与 [`insert_row`] 同源）。
/// 不排序：排序知识单点在 [`super::engine::total_order`]（read_ops 调用方消费）。
pub(crate) fn read_all(conn: &Connection) -> Result<Vec<SyncOp>> {
    let mut stmt =
        conn.prepare("SELECT op_id, device_id, clock, schema_version, payload FROM sync_ops")?;
    let rows = stmt.query_map([], map_op_row)?;
    let mut ops = Vec::new();
    for row in rows {
        ops.push(op_from_row(row?));
    }
    ops.into_iter().collect::<Result<Vec<_>>>()
}

/// 读取本机产出且时钟晚于 `after_clock` 的 op（通道上传接缝：只发布自己流，
/// 按时钟序返回；通道上传位点以 manifest 为权威，见 `channel::publish_own_ops`）。
pub(crate) fn read_own_since(
    conn: &Connection,
    device_id: &str,
    after_clock: i64,
) -> Result<Vec<SyncOp>> {
    let mut stmt = conn.prepare(
        "SELECT op_id, device_id, clock, schema_version, payload FROM sync_ops \
         WHERE device_id = ?1 AND clock > ?2 ORDER BY clock ASC",
    )?;
    let rows = stmt.query_map(params![device_id, after_clock], map_op_row)?;
    let mut ops = Vec::new();
    for row in rows {
        ops.push(op_from_row(row?));
    }
    ops.into_iter().collect::<Result<Vec<_>>>()
}

/// op 行读取映射（[`read_all`] / [`read_own_since`] 共用；反序列化失败属程序
/// 缺陷，fail loud——wire 侧不可解析载荷由 [`super::engine::ingest_ops`] 挂起承接）。
fn map_op_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, i64, i64, String)> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
}

/// 行五元组 → op 信封（载荷反序列化同源单点）。
fn op_from_row(row: (String, String, i64, i64, String)) -> Result<SyncOp> {
    let (op_id, device_id, clock, schema_version, payload) = row;
    let command = serde_json::from_str(&payload)
        .map_err(|e| AppError::Invalid(format!("op 载荷反序列化失败: {e}")))?;
    Ok(SyncOp {
        op_id,
        device_id,
        clock,
        schema_version,
        command,
    })
}
