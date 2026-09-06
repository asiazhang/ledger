//! 计划来源反查（spec #704 / issue #707 交易来源列）：按生成交易 id 反查期次 →
//! 计划，供核心交易域列表/搜索读路径按页填充来源列（展示名 + 计划状态）。
//! 只读展示反查，不新增数据级反向引用（期次表 `transaction_id` 既有指针即为通道）。

use rusqlite::Connection;

use super::models::ScheduledKind;
use crate::db::query::{FromRow, query_all};
use crate::error::Result;

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
