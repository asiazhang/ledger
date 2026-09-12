//! 计划来源反查（spec #704 / issue #707 交易来源列）：按生成交易 id 反查期次 →
//! 计划，供核心交易域列表/搜索读路径按页填充来源列（展示名 + 计划状态）。
//! 只读展示反查，不新增数据级反向引用（期次表 `transaction_id` 既有指针即为通道）。
//!
//! 自 #1090 起本模块私有化并承担接缝反转的实现侧：核心交易域只定义注册点
//! （`transaction::read::register_plan_source_resolver`），本模块提供实现并经
//! [`install_plan_source_hook`] 在壳层启动时装入——核心交易域对定时计划域零直接
//! 依赖（域间禁边，`scripts/check-structure.ts`）。反查行不再公开再导出。

use std::collections::HashMap;

use rusqlite::Connection;

use super::models::ScheduledKind;
use crate::db::query::{FromRow, query_all};
use crate::error::Result;
use crate::transaction::{TransactionSource, TransactionSourceKind, TransactionSourceStatus};

/// 计划来源展示行（以生成交易 id 为键）：计划 id + 形态 + 状态 + 备注。
/// `status`/`note` 沿用域核心模型的 String/Option 形态（wire 与裸列一致）。
#[derive(Debug, Clone)]
pub struct PlanSourceDisplay {
    /// 生成该交易的期次所链接的交易 id（反查键）。
    pub transaction_id: String,
    /// 计划 id（来源实体 id）。
    pub plan_id: String,
    /// 计划形态（分期/订阅/定时转账 → 来源类型三枚举）。
    pub kind: ScheduledKind,
    /// 计划状态（`cancelled` → 来源状态「已取消」标注；其余状态不标注）。
    pub status: String,
    /// 计划备注（展示名 = 计划名口径；可空，空由前端按类型名兜底）。
    pub note: Option<String>,
}

impl FromRow for PlanSourceDisplay {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PlanSourceDisplay {
            transaction_id: row.get(0)?,
            plan_id: row.get(1)?,
            kind: row.get(2)?,
            status: row.get(3)?,
            note: row.get(4)?,
        })
    }
}

/// 按生成交易 id 批量反查计划来源展示行（spec #704 / issue #707）：供核心交易域
/// 按页填充来源列（收集页内无来源交易 id 后一次查询，不做逐行 N+1）。
/// 反查通道为期次表 `transaction_id`（生成时写入、交易删除置空），唯一索引
/// `idx_scheduled_occurrences_txn` 保证一交易至多一行，缺失即无计划来源。
/// 计划无删除路径，不做 `is_deleted` 过滤（词汇表「来源列」：已完成期次的交易
/// 链接永不断）。
pub fn source_display_by_transaction_ids(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<Vec<PlanSourceDisplay>> {
    if transaction_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; transaction_ids.len()].join(",");
    query_all(
        conn,
        &format!(
            "SELECT o.transaction_id, p.id, p.kind, p.status, p.note \
             FROM scheduled_transaction_occurrences o \
             JOIN scheduled_transactions p ON p.id = o.scheduled_transaction_id \
             WHERE o.transaction_id IN ({placeholders})"
        ),
        rusqlite::params_from_iter(transaction_ids.iter()),
    )
}

/// 把计划来源解析实现注册进核心交易域的注册点（幂等，进程级一次）。
///
/// 接缝形态（issue #1090 / spec #1086）：下层（核心交易域）定义注册点、上层（本域）
/// 提供实现、壳层启动时接线——`lib.rs::run` 与测试建库单点（`test_support::open`、
/// BDD world）各调用一次，生产与测试同形。
pub fn install_plan_source_hook() {
    crate::transaction::read::register_plan_source_resolver(plan_source_hook);
}

/// 计划来源解析接缝的实现（[`install_plan_source_hook`] 的注册体）：反查行
/// （[`source_display_by_transaction_ids`]）→ 交易域来源模型的字段映射。
/// 口径与迁移前交易域内联映射逐字段一致（spec #704）：kind 三枚举 → 来源类型
/// 三枚举；`cancelled` 计划状态 → Cancelled 标注；展示名 = 计划名（备注，无备注
/// 回空串，前端按类型名兑底）。
fn plan_source_hook(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    Ok(source_display_by_transaction_ids(conn, transaction_ids)?
        .into_iter()
        .map(|row| {
            let source = TransactionSource {
                kind: match row.kind {
                    ScheduledKind::Installment => TransactionSourceKind::InstallmentPlan,
                    ScheduledKind::Subscription => TransactionSourceKind::Subscription,
                    ScheduledKind::ScheduledTransfer => TransactionSourceKind::ScheduledTransfer,
                },
                entity_id: row.plan_id,
                display_name: row.note.unwrap_or_default(),
                status: (row.status == "cancelled").then_some(TransactionSourceStatus::Cancelled),
            };
            (row.transaction_id, source)
        })
        .collect())
}
