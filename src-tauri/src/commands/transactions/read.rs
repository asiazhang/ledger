use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::db::query::{query_all, query_one};
use crate::db::{device_id, now_iso};
use crate::error::{AppError, Result};
use crate::models::{
    NormalizedTransaction, Transaction, TransactionListFilter, TransactionListResult,
};

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
    if let Some(kind) = filter.kind.as_deref() {
        where_clause.push_str(" AND kind = ?");
        params.push(kind.to_string());
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
         to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted \
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
    let items = query_all(conn, &sql, rusqlite::params_from_iter(params))?;
    Ok(TransactionListResult { items, total })
}

/// 按 `id` 读取未删除交易，供修改接口返回更新后的完整交易。不存在返回 `NotFound`。
pub fn get_transaction_internal(conn: &Connection, id: &str) -> Result<Transaction> {
    query_one::<Transaction, _>(
        conn,
        "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,\
         version,device_id,is_deleted FROM transactions WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
    )?
    .ok_or_else(|| AppError::NotFound(format!("交易不存在: {id}")))
}

/// 更新交易行字段（创建与修改共用的归一化字段），保留 `id`、`created_at` 与去重身份
/// （`idempotency_key` / `dedup_hash`），版本号递增。buy/sell 同样经由本函数落交易行字段。
pub(crate) fn update_transaction_row(
    conn: &Connection,
    id: &str,
    norm: &NormalizedTransaction,
) -> Result<()> {
    conn.execute(
        "UPDATE transactions \
         SET kind=?2, amount_cents=?3, currency_code=?4, amount_native_cents=?5, account_id=?6, \
         to_account_id=?7, category_id=?8, refund_of_transaction_id=?9, note=?10, date=?11, \
         updated_at=?12, version=version+1, device_id=?13 \
         WHERE id=?1",
        rusqlite::params![
            id,
            norm.kind,
            norm.amount_cents,
            norm.currency_code,
            norm.amount_native_cents,
            norm.account_id,
            norm.to_account_id,
            norm.category_id,
            norm.refund_of_transaction_id,
            norm.note,
            norm.date,
            now_iso(),
            device_id(),
        ],
    )?;
    Ok(())
}

/// 删除交易（软删除 `is_deleted=1`）。
///
/// buy 交易同步清理关联持仓（`security_lots` / `security_transactions`）：
/// 若该买入已有部分卖出（`remaining_quantity < initial_quantity`）则拒绝删除。
/// 不存在的 id 返回 `AppError::NotFound`（HTTP 侧映射 404）。IPC 与 HTTP 端点共用本函数。
pub fn delete_transaction_internal(conn: &Connection, id: &str) -> Result<()> {
    let is_buy: bool = conn
        .query_row(
            "SELECT kind='buy' FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get::<_, i64>(0).map(|v| v != 0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("交易不存在: {id}")))?;

    if is_buy {
        // 守卫（部分卖出拒绝）与持仓/卖出关联清理与按 id 修改共用（见 #50）。
        crate::commands::investment::cleanup_buy_side_effects(
            conn,
            id,
            "该买入交易已有部分卖出，无法删除",
        )?;
    }

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    // 索引维护由后台定时刷新承担：触发器（trg_search_enqueue_txn_update）已入队
    // `search_reindex_queue`，软删除后到下次刷新前该交易仍可能被搜到（时效性要求低，
    // 可接受，见 ADR-0004 决策 #14）。
    Ok(())
}
