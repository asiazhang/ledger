//! ParkedOp 挂起队列（issue #856 / ADR-0091 决策 6）：不可重放 op 的统一归宿。
//!
//! 覆盖两类场景：外键依赖失败（如 A 端删除账户、B 端往该账户记账）与 schema
//! 版本偏斜双向（旧命令重放到新 schema、旧端收到新命令）。挂起 = 落
//! `sync_parked_ops`、**不落** `sync_ops`（未应用）、以码化原因通知用户裁决，
//! 不阻塞其余 op 重放，不静默丢弃、不自动复活数据。
//!
//! 出队由重投递自然承接：挂起 op 不在日志中，通道重投递（升级后重试、依赖方
//! 补齐后重试）会再次进入重放——成功即出队（同 `op_id` 删除挂起行）；仍失败
//! 则按 `op_id` 幂等覆盖挂起行（重复投递不堆积）。本模块是挂起队列的唯一
//! SQL 收口。

use rusqlite::params;

use crate::db::now_iso;
use crate::error::Result;

/// 挂起操作（ParkedOp）：op 信封原样保存 + 码化挂起原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParkedOp {
    /// op 标识；信封不可读时为合成 id（`parked-` 前缀 + UUID）。
    pub op_id: String,
    /// 来源设备标识（信封不可读时为空串）。
    pub device_id: String,
    /// 来源端内逻辑时钟（信封不可读时为 0）。
    pub clock: i64,
    /// 产生时 schema 版本（信封不可读时为 0）。
    pub schema_version: i64,
    /// 实体判别键（载荷不可解时按 wire tag 尽力提取，否则空串）。
    pub entity: String,
    /// 实体 id（不可知时为空串）。
    pub entity_id: String,
    /// op 载荷原样（wire JSON；供用户裁决与升级后重放）。
    pub payload: String,
    /// 码化挂起原因（稳定错误码）。
    pub code: String,
    /// 码化挂起原因的插值参数（按消息中动态值出现顺序；与 `code` 同源于
    /// `AppError::Coded`，ADR-0050）。前端按 `errors.<code>` 模板插值用。
    pub params: Vec<String>,
    /// 中文详情（**已渲染**的完整句，与错误消息纪律同款）：码命中模板且
    /// `params` 齐备时前端用模板渲染，否则降级透传本字段（恒可读）。
    pub message: String,
    /// 挂起时刻（本机簿记事实）。
    pub parked_at: String,
}

/// 挂起码常量：schema 版本偏斜——op 产生自更新版本（旧端收到新命令）。
pub(super) const CODE_SCHEMA_AHEAD: &str = "sync-engine.schema-ahead";
/// 挂起码常量：wire 载荷无法解码（旧命令偏斜或信封损坏）。
pub(super) const CODE_UNDECODABLE: &str = "sync-engine.op-undecodable";
/// 挂起码常量：重放执行失败且无码化原因可携带（外键依赖失败等业务拒绝优先
/// 原样携带其码）。
pub(super) const CODE_REPLAY_FAILED: &str = "sync-engine.replay-failed";

/// 挂起入队（按 `op_id` 幂等覆盖：重复投递同一 op 不堆积、原因随最新一次刷新）。
pub(super) fn park(conn: &rusqlite::Connection, op: &ParkedOp) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_parked_ops \
         (op_id, device_id, clock, schema_version, entity, entity_id, payload, park_code, park_params, park_message, parked_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
         ON CONFLICT(op_id) DO UPDATE SET \
           park_code = excluded.park_code, park_params = excluded.park_params, \
           park_message = excluded.park_message, parked_at = excluded.parked_at",
        params![
            op.op_id,
            op.device_id,
            op.clock,
            op.schema_version,
            op.entity,
            op.entity_id,
            op.payload,
            op.code,
            encode_params(&op.params),
            op.message,
            now_iso(),
        ],
    )?;
    Ok(())
}

/// 出队（重放成功后调用；无挂起行时静默——本地产出/无历史挂起属常态）。
pub(super) fn resolve(conn: &rusqlite::Connection, op_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM sync_parked_ops WHERE op_id = ?1",
        params![op_id],
    )?;
    Ok(())
}

/// 挂起队列是否为空（引导守卫用：目标已有挂起即已参与同步）。
pub(super) fn is_empty(conn: &rusqlite::Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sync_parked_ops", [], |r| r.get(0))?;
    Ok(count == 0)
}

/// 挂起清单（按全序返回：挂起通知与裁决界面的数据面）。
pub(super) fn list(conn: &rusqlite::Connection) -> Result<Vec<ParkedOp>> {
    let mut stmt = conn.prepare(
        "SELECT op_id, device_id, clock, schema_version, entity, entity_id, payload, \
         park_code, park_params, park_message, parked_at \
         FROM sync_parked_ops ORDER BY clock ASC, device_id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ParkedOp {
            op_id: r.get(0)?,
            device_id: r.get(1)?,
            clock: r.get(2)?,
            schema_version: r.get(3)?,
            entity: r.get(4)?,
            entity_id: r.get(5)?,
            payload: r.get(6)?,
            code: r.get(7)?,
            params: decode_params(r.get::<_, String>(8)?),
            message: r.get(9)?,
            parked_at: r.get(10)?,
        })
    })?;
    let mut ops = Vec::new();
    for row in rows {
        ops.push(row?);
    }
    Ok(ops)
}

/// `params` 落库编码：JSON 字符串数组文本（与同表 `payload` 同款；params 恒整读
/// 整写、不参与查询，无需子表）。
fn encode_params(params: &[String]) -> String {
    serde_json::to_string(params).unwrap_or_else(|_| "[]".to_string())
}

/// `params` 出库解码：坏值降级空数组（params 缺失只影响插值，不影响挂起行本身
/// 的可用性——前端会回退到 `message` 透传，不静默丢弃挂起通知）。
fn decode_params(raw: String) -> Vec<String> {
    serde_json::from_str(&raw).unwrap_or_default()
}
