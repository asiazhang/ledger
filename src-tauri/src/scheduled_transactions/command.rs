//! 定时计划同步命令（issue #856 / #860 / ADR-0091 决策 5）：期次触发与计划 CRUD
//! 命令的载荷形态、产出单点与重放执行。
//!
//! - **载荷形态**（[`ScheduledCommand`]）：
//!   - 期次触发：落地该期交易并完成重放端对应期次。期次身份是 OccurrenceKey
//!     （plan_id + 期次标识 = 期次计划日期，跨端确定性；本地期次行 id 各端独立
//!     生成、不作身份），落地身份是键派生的确定性交易 id
//!     ([`occurrence_transaction_id`])——两台设备同时自动执行同一期派生同一 id，
//!     天然只落一次。行载荷与交易命令同构（[`NormalizedTransaction`]，含源端
//!     折算，重放不依赖本地汇率表，ADR-0091 决策 3）。
//!   - 计划 CRUD（issue #860）：建档携带实体 id + 全量入参（重放端跑同一校验
//!     协议并本地展开期次行）；状态变更携带落定状态（取消的期次级联副作用随
//!     同一协议重放）；订阅编辑携带解决后的非金额字段；期次展开按本端现状
//!     重推（count 守卫防溢出）。
//!
//!   **只增不改**：新命令只追加变体。
//! - **产出单点**（[`record_local`]）：期次执行入口（引擎单期执行）与计划写编排
//!   入口（建档 / 状态变更 / 订阅编辑 / 期次展开）成功后调用，op 随执行事务
//!   提交/回滚。
//! - **重放执行**（[`replay_command`]）：期次触发按确定性落地身份幂等；计划
//!   CRUD 与本地写同一执行协议（校验 + 落库），不产出 op——外来 op 由同步引擎
//!   落日志。

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::db::{deterministic_uuid, now_iso};
use crate::error::Result;
use crate::sync_engine::command::ReplayEffect;
use crate::sync_engine::device_id;
use crate::sync_engine::{DomainCommand, record_local as record_op};
use crate::transaction::NormalizedTransaction;
use crate::transaction::writer;

use super::models::{CreateScheduledInput, ScheduledStatus, UpdateSubscriptionInput};

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
    /// 建档（issue #860）：实体 id 与全量入参随命令携带；重放端跑同一校验协议
    /// （账户存活、商户/保单在用等）并本地展开期次行（行 id 各端独立生成）。
    CreatePlan {
        id: String,
        input: CreateScheduledInput,
    },
    /// 状态变更（issue #860）：落定状态随命令携带；取消的期次级联副作用随
    /// 同一协议在重放端生效。
    UpdatePlanStatus {
        id: String,
        new_status: ScheduledStatus,
    },
    /// 订阅编辑（issue #860）：解决后的非金额字段随命令携带（全量替换语义，
    /// 与本地编辑同语义；金额不可编辑哨兵在源端已裁决，不随行携带）。
    UpdateSubscription {
        id: String,
        account_id: String,
        category_id: Option<String>,
        note: Option<String>,
        merchant_id: Option<String>,
    },
    /// 期次展开（issue #860）：重放端按本端现状重推同一展开协议（count 守卫
    /// 防溢出），期次行 id 各端独立生成。
    ExpandOccurrences { plan_id: String },
}

impl ScheduledCommand {
    /// 命令指向的实体键（LWW 裁决域 = 单个计划，ADR-0091 决策 4）：计划 CRUD
    /// 携带 plan id；期次触发冲突域在 OccurrenceKey、期次展开按本端现状重推，
    /// 均无实体指向、不参与同实体 LWW（ADR-0091 决策 5）。实体标签不在此返回
    /// ——由同步域重放注册表单源组装（ADR-0101 勘误 3）。
    pub fn subject(&self) -> Option<&str> {
        match self {
            ScheduledCommand::ExecuteOccurrence { .. }
            | ScheduledCommand::ExpandOccurrences { .. } => None,
            ScheduledCommand::CreatePlan { id, .. }
            | ScheduledCommand::UpdatePlanStatus { id, .. }
            | ScheduledCommand::UpdateSubscription { id, .. } => Some(id),
        }
    }

    /// 落地身份（OccurrenceKey 派生，产出与重放两端各自派生、不随行携带——
    /// 消除第二事实源漂移；同键跨端恒同值，防双扣的结构承载）。仅期次触发
    /// 命令有落地身份，其余变体无此概念。
    pub fn landing_transaction_id(&self) -> String {
        match self {
            ScheduledCommand::ExecuteOccurrence {
                plan_id,
                scheduled_date,
                ..
            } => occurrence_transaction_id(plan_id, scheduled_date),
            _ => String::new(),
        }
    }
}

/// OccurrenceKey → 确定性落地身份：同键跨端恒同值（防双扣的结构承载）。
pub fn occurrence_transaction_id(plan_id: &str, scheduled_date: &str) -> String {
    deterministic_uuid(&format!("scheduled-occurrence|{plan_id}|{scheduled_date}"))
}

/// op 产出接缝（定时计划域集中单点）：期次执行 / 计划写成功后追加一条 op 进
/// 本机 OpLog。
///
/// 仅期次执行入口与计划写编排入口调用；随执行事务提交/回滚，写失败不残留 op。
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

/// 同步重放入口：执行外来定时计划命令（issue #856 / #860 / ADR-0091）。
///
/// 期次触发：落地身份已存在（双端同时执行的另一端产出，或本端此前已重放）即
/// 幂等命中（`IdempotentHit`，引擎报告 Deduped）；否则按行落库（余额缓存重算在
/// Writer 接缝内）并完成本地期次，返回 `Applied`。计划 CRUD：与本地写同一执行
/// 协议（校验 + 落库），恒 `Applied`。**不产出 op**：外来 op 由同步引擎在重放
/// 事务内落日志。
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
            // 定时计划不携带资金账户（funding_account_id 恒 None），与 engine 落地路径一致。
            writer::validate_accounts_alive(
                conn,
                &row.account_id,
                row.to_account_id.as_deref(),
                None,
            )?;
            let norm = writer::NormalizedRow::try_from(row)?;
            writer::insert_row_with_id(conn, &landing_id, &norm)?;
            complete_local_occurrence(conn, plan_id, scheduled_date, &landing_id)?;
            Ok(ReplayEffect::Applied)
        }
        ScheduledCommand::CreatePlan { id, input } => {
            super::engine::create_protocol(conn, id, input)?;
            Ok(ReplayEffect::Applied)
        }
        ScheduledCommand::UpdatePlanStatus { id, new_status } => {
            super::engine::update_plan_status_protocol(conn, id, *new_status)?;
            Ok(ReplayEffect::Applied)
        }
        ScheduledCommand::UpdateSubscription {
            id,
            account_id,
            category_id,
            note,
            merchant_id,
        } => {
            // 金额哨兵恒 false：改价在源端已被显式拒绝，能产出编辑 op 的输入必然
            // 不携带金额字段（ADR-0023 决策三）。
            super::engine::update_subscription_protocol(
                conn,
                &UpdateSubscriptionInput {
                    id: id.clone(),
                    account_id: account_id.clone(),
                    category_id: category_id.clone(),
                    note: note.clone(),
                    merchant_id: merchant_id.clone(),
                    amount_cents: false,
                    total_amount_cents: false,
                },
            )?;
            Ok(ReplayEffect::Applied)
        }
        ScheduledCommand::ExpandOccurrences { plan_id } => {
            super::engine::expand_occurrences_protocol(conn, plan_id)?;
            Ok(ReplayEffect::Applied)
        }
    }
}
