use std::collections::HashMap;

use rusqlite::Connection;

use super::model::{
    Transaction, TransactionListFilter, TransactionListResult, TransactionSource,
    TransactionSourceKind, TransactionSourceStatus,
};
use crate::db::query::{query_all, query_one};
use crate::error::{AppError, Result};
use crate::policy;
use crate::scheduled_transactions::{self, ScheduledKind};

pub use get_transaction_internal as get_transaction;
pub use list_transactions_internal as list_transactions;

/// 按页填充来源列（spec #704，词汇表「来源列」来源判定优先级：保单直挂 >
/// 计划反查 > 物品反查 > 标的反查），逐级只对尚无来源的行填充，不做逐行 N+1：
/// ① 保单直挂（issue #706）：保单 id 已在行内（PolicyReference），去重后一次
///    批量反查（`policy::source_display_by_ids`）。双挂场景在此天然优先——
///    订阅期次执行时把协议上的保单引用复制进流水，自动保费流水同时有
///    「订阅协议 + 保单」两条线索，来源列显示保单（更具体的档案）。
/// ② 计划反查（issue #707）：按生成交易 id 批量反查期次 → 计划
///    （`scheduled_transactions::source_display_by_transaction_ids`），展示名 =
///    计划名（备注，可空由前端按类型名兜底），已取消计划携带状态标注。
///
/// 零迁移：来源是读时推导，不落库；无来源交易（手动录入/AI 导入）原样为 `None`。
pub(super) fn attach_sources(conn: &Connection, items: &mut [Transaction]) -> Result<()> {
    // ① 保单直挂（保单 id 已在行内）
    let mut policy_ids: Vec<String> = Vec::new();
    for txn in items.iter() {
        if let Some(pid) = txn.policy_id.as_ref()
            && !policy_ids.contains(pid)
        {
            policy_ids.push(pid.clone());
        }
    }
    if !policy_ids.is_empty() {
        let refs = policy::source_display_by_ids(conn, &policy_ids)?;
        let by_id: HashMap<&str, &policy::PolicySourceDisplay> =
            refs.iter().map(|r| (r.id.as_str(), r)).collect();
        for txn in items.iter_mut() {
            let Some(pid) = txn.policy_id.as_deref() else {
                continue;
            };
            let Some(reference) = by_id.get(pid) else {
                // 引用完整性由外键（ON DELETE RESTRICT）保证；缺行属防御性跳过，
                // 不虚构展示名也不中断整页读取。
                continue;
            };
            txn.source = Some(TransactionSource {
                kind: TransactionSourceKind::Policy,
                entity_id: pid.to_string(),
                display_name: reference.product_name.clone(),
                status: reference
                    .is_deleted
                    .then_some(TransactionSourceStatus::Deleted),
            });
        }
    }

    // ② 计划反查（仅对保单未命中的行；期次唯一索引保证一交易至多一行）
    let plan_txn_ids: Vec<String> = items
        .iter()
        .filter(|t| t.source.is_none())
        .map(|t| t.id.clone())
        .collect();
    if !plan_txn_ids.is_empty() {
        let rows = scheduled_transactions::source_display_by_transaction_ids(conn, &plan_txn_ids)?;
        let by_txn: HashMap<&str, &scheduled_transactions::PlanSourceDisplay> = rows
            .iter()
            .map(|r| (r.transaction_id.as_str(), r))
            .collect();
        for txn in items.iter_mut() {
            if txn.source.is_some() {
                continue;
            }
            let Some(row) = by_txn.get(txn.id.as_str()) else {
                continue;
            };
            txn.source = Some(TransactionSource {
                kind: match row.kind {
                    ScheduledKind::Installment => TransactionSourceKind::InstallmentPlan,
                    ScheduledKind::Subscription => TransactionSourceKind::Subscription,
                    ScheduledKind::ScheduledTransfer => TransactionSourceKind::ScheduledTransfer,
                },
                entity_id: row.plan_id.clone(),
                // 展示名 = 计划名（备注）；无备注回空串，前端按类型名兜底展示。
                display_name: row.note.clone().unwrap_or_default(),
                status: (row.status == "cancelled").then_some(TransactionSourceStatus::Cancelled),
            });
        }
    }
    Ok(())
}

pub fn list_transactions_internal(
    conn: &Connection,
    filter: &TransactionListFilter,
) -> Result<TransactionListResult> {
    // 过滤条件与 total/items 共用同一 WHERE 子句，保证 total 恒为"满足过滤条件的未删除交易总数"。
    let mut where_clause = String::from("WHERE is_deleted=0");
    let mut params: Vec<String> = Vec::new();
    if let Some(from) = filter.from.as_deref() {
        where_clause.push_str(" AND date >= ?");
        params.push(from.to_string());
    }
    if let Some(to) = filter.to.as_deref() {
        where_clause.push_str(" AND date <= ?");
        params.push(to.to_string());
    }
    if let Some(account_id) = filter.account_id.as_deref() {
        where_clause.push_str(" AND account_id = ?");
        params.push(account_id.to_string());
    }
    if let Some(account_id) = filter.involving_account_id.as_deref() {
        where_clause.push_str(" AND (account_id = ? OR to_account_id = ?)");
        params.push(account_id.to_string());
        params.push(account_id.to_string());
    }
    if let Some(merchant_id) = filter.merchant_id.as_deref() {
        where_clause.push_str(" AND merchant_id = ?");
        params.push(merchant_id.to_string());
    }
    // 分类下钻两字段（issue #377）：精确匹配不含子分类；仅无分类命中 category_id IS NULL。
    // 与其他维度同规：AND 组合，互不隐含。
    if let Some(category_id) = filter.category_id.as_deref() {
        where_clause.push_str(" AND category_id = ?");
        params.push(category_id.to_string());
    }
    if filter.uncategorized_only == Some(true) {
        where_clause.push_str(" AND category_id IS NULL");
    }
    if let Some(kind) = filter.kind {
        where_clause.push_str(" AND kind = ?");
        params.push(kind.as_str().to_string());
    }
    // 类型集合过滤（issue #581 报表分类下钻载荷）：kind IN (...)，与其余维度 AND 组合；
    // 与单值 kind 同携时同样 AND（交集语义），已发布单值参数语义不变（只增不改）；
    // 空集合视为未携带（不过滤），先例同 uncategorized_only=false。
    if let Some(kinds) = filter.kinds.as_ref().filter(|k| !k.is_empty()) {
        let placeholders = vec!["?"; kinds.len()].join(",");
        where_clause.push_str(&format!(" AND kind IN ({placeholders})"));
        kinds
            .iter()
            .for_each(|k| params.push(k.as_str().to_string()));
    }

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM transactions {where_clause}"),
        rusqlite::params_from_iter(params.iter()),
        |r| r.get(0),
    )?;

    // 确定性排序：date DESC, created_at DESC, id DESC。
    // id 是最终 tiebreaker——`now_iso()` 为秒级精度，同一秒内写入的行 created_at 相同，
    // 不加 id 翻页会漂移（重复/遗漏）。
    let mut sql = format!(
        "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted,merchant_id,policy_id \
         FROM transactions {where_clause} ORDER BY date DESC, created_at DESC, id DESC"
    );
    // 分页路径优先：传 page_size 时按 offset 页码取当前页（小于 1 按 1 处理，
    // 与 InstrumentListFilter 先例一致；offset 用 saturating 运算防溢出）；
    // 否则 limit 路径取前 N 条（沿用 SQLite 原生语义：LIMIT 0 返回空、负值无上限）；
    // 两者都缺省时返回全部（total 恒返回）。
    if let Some(page_size) = filter.page_size {
        // 钳制到 SQLite 可接受的 64 位整数范围，防止极端输入（usize::MAX）产生
        // "datatype mismatch" 或 debug 构建 panic。
        let page_size = i64::try_from(page_size.max(1)).unwrap_or(i64::MAX);
        let page = filter.page.unwrap_or(1).max(1);
        let offset = i64::try_from(page.saturating_sub(1).saturating_mul(page_size as usize))
            .unwrap_or(i64::MAX);
        sql.push_str(&format!(" LIMIT {page_size} OFFSET {offset}"));
    } else if let Some(n) = filter.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    let mut items = query_all(conn, &sql, rusqlite::params_from_iter(params))?;
    attach_sources(conn, &mut items)?;
    Ok(TransactionListResult { items, total })
}

/// 按 `id` 读取未删除交易，供修改接口返回更新后的完整交易。不存在返回 `NotFound`。
pub fn get_transaction_internal(conn: &Connection, id: &str) -> Result<Transaction> {
    query_one::<Transaction, _>(
        conn,
        "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,\
         version,device_id,is_deleted,merchant_id,policy_id FROM transactions WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
    })
}
