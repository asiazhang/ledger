//! 定时计划同步命令（issue #856 / ADR-0091 决策 5）：期次触发命令的载荷形态、
//! 产出单点与重放执行。
//!
//! - **载荷形态**（[`ScheduledCommand`]）：期次触发的语义命令——落地该期交易并
//!   完成重放端对应期次。期次身份是 OccurrenceKey（plan_id + 期次标识 = 期次
//!   计划日期，跨端确定性；本地期次行 id 各端独立生成、不作身份），落地身份是
//!   键派生的确定性交易 id（[`occurrence_transaction_id`]）——两台设备同时自动
//!   执行同一期派生同一 id，天然只落一次。行载荷与交易命令同构
//!   （[`NormalizedTransaction`]，含源端折算，重放不依赖本地汇率表，ADR-0091
//!   决策 3）。**只增不改**：新命令只追加变体。
//! - **产出单点**（[`record_local`]）：期次执行入口（引擎单期执行）成功后调用，
//!   op 随执行事务提交/回滚。
//! - **重放执行**（[`replay_command`]):确定性落地身份已存在（含软删——不自动
//!   复活）即幂等命中；否则按行落库（含余额缓存重算）并按 (plan_id, 计划日期)
//!   完成本地期次（本地无对应期次行时只落交易，期次行可晚于落地到达）。

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::db::{deterministic_uuid, now_iso};
use crate::error::Result;
use crate::sync_engine::device_id;
use crate::sync_engine::engine::ReplayEffect;
use crate::sync_engine::{DomainCommand, record_local as record_op};
use crate::transaction::NormalizedTransaction;
use crate::transaction::writer;

/// 定时计划同步命令（serde：`action` 判别；作为 DomainCommand 信封的 payload 内嵌）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ScheduledCommand {
    /// 期次触发：落地该期交易并完成重放端对应期次（OccurrenceKey 幂等）。
    ExecuteOccurrence {
        /// 所属计划 id（键前半；计划行随 #860 同步，两端一致）。
        plan_id: String,
        /// 期次标识 = 期次计划日期（键后半；期次语义见定时计划域 Occurrence，
        /// 核心域交易 date 直接复用计划日期，跨端确定性身份）。
        scheduled_date: String,
        /// 归一化交易行（含源端折算结果，与交易命令同构）。
        row: NormalizedTransaction,
    },
}

impl ScheduledCommand {
    /// 落地身份（OccurrenceKey 派生，产出与重放两端各自派生、不随行携带——
    /// 消除第二事实源漂移；同键跨端恒同值，防双扣的结构承载）。
    pub fn landing_transaction_id(&self) -> String {
        match self {
            ScheduledCommand::ExecuteOccurrence {
                plan_id,
                scheduled_date,
                ..
            } => occurrence_transaction_id(plan_id, scheduled_date),
        }
    }
}

/// OccurrenceKey → 确定性落地身份：同键跨端恒同值（防双扣的结构承载）。
pub fn occurrence_transaction_id(plan_id: &str, scheduled_date: &str) -> String {
    deterministic_uuid(&format!("scheduled-occurrence|{plan_id}|{scheduled_date}"))
}

/// op 产出接缝（定时计划域集中单点）：期次执行成功后追加一条 op 进本机 OpLog。
///
/// 仅期次执行入口调用；随执行事务提交/回滚，写失败不残留 op。
pub(crate) fn record_local(conn: &Connection, command: ScheduledCommand) -> Result<()> {
    record_op(conn, DomainCommand::Scheduled(command))?;
    Ok(())
}

/// 落地身份是否已存在（含软删行：已落期次被用户删除后，重放不自动复活）。
pub(crate) fn transaction_landed(conn: &Connection, transaction_id: &str) -> Result<bool> {
    let hit: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM transactions WHERE id = ?1",
            params![transaction_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(hit.is_some())
}

/// 按 (plan_id, 计划日期) 完成本地期次并回填落地交易。
///
/// 本地无对应 pending/failed 期次行时静默（期次行可晚于落地到达，#860 同步后
/// 各端期次行收敛）；幂等重放不产生第二次状态变更。
fn complete_local_occurrence(
    conn: &Connection,
    plan_id: &str,
    scheduled_date: &str,
    transaction_id: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE scheduled_transaction_occurrences \
         SET status='completed', transaction_id=?3, updated_at=?4, version=version+1, device_id=?5 \
         WHERE scheduled_transaction_id=?1 AND scheduled_date=?2 \
           AND status IN ('pending','failed') AND is_deleted=0",
        params![
            plan_id,
            scheduled_date,
            transaction_id,
            now_iso(),
            device_id(conn)?
        ],
    )?;
    Ok(())
}

/// 同步重放入口：执行外来期次触发命令（issue #856 / ADR-0091 决策 5）。
///
/// 落地身份已存在（双端同时执行的另一端产出，或本端此前已重放）即幂等命中
/// （`IdempotentHit`，引擎报告 Deduped）；否则按行落库（余额缓存重算在 Writer
/// 接缝内）并完成本地期次，返回 `Applied`。**不产出 op**：外来 op 由同步引擎
/// 在重放事务内落日志。
pub(crate) fn replay_command(
    conn: &Connection,
    command: &ScheduledCommand,
) -> Result<ReplayEffect> {
    match command {
        ScheduledCommand::ExecuteOccurrence {
            plan_id,
            scheduled_date,
            row,
        } => {
            let landing_id = occurrence_transaction_id(plan_id, scheduled_date);
            if transaction_landed(conn, &landing_id)? {
                return Ok(ReplayEffect::IdempotentHit);
            }
            // 账户存活守卫（issue #856）：往已删账户记账不落地，挂起待裁决。
            writer::validate_accounts_alive(conn, &row.account_id, row.to_account_id.as_deref())?;
            let norm = writer::NormalizedRow::try_from(row)?;
            writer::insert_row_with_id(conn, &landing_id, &norm)?;
            complete_local_occurrence(conn, plan_id, scheduled_date, &landing_id)?;
            Ok(ReplayEffect::Applied)
        }
    }
}
