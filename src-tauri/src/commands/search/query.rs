//! 查询执行：服务端分页搜索内部实现（FTS MATCH + 可选金额/日期筛选）。

use rusqlite::Connection;

use crate::db::query::query_all;
use crate::error::Result;
use crate::models::TransactionSearchResult;

use super::index::reindex_queue_pending;
use super::text::build_match_query;

/// 每页条数上限（防呆，防止极端输入拖垮查询）。
const MAX_PAGE_SIZE: usize = 200;

/// 服务端分页搜索交易。整词/前缀匹配（FTS5 MATCH），JOIN 回主表过滤软删除，
/// 排序按相关度 rank 优先、日期倒序次之、id 兜底（与交易列表先例一致，防同秒
/// 批量写入翻页漂移）；返回当前页与命中总数。
///
/// 支持可选筛选（与关键字 AND 组合，全部可省略、单边可用）：
/// - `amount_min_cents` / `amount_max_cents`：金额区间（整数分，含边界；按原始
///   币种分值过滤，MVP 阶段与 `amount_native_cents` 1:1）；
/// - `date_from` / `date_to`：日期区间（`YYYY-MM-DD` 字符串比较，含边界）。
///
/// 空查询（无关键字）时：有筛选 → 执行仅筛选查询；无筛选 → 维持返回空结果。
///
/// 参数较多（8 个）是 issue #40 规格要求的签名（四个可选筛选参数直传，BDD/单测
/// 沿用直调内部函数模式），故显式 allow `too_many_arguments`。
#[allow(clippy::too_many_arguments)]
pub fn search_transactions_internal(
    conn: &Connection,
    query: &str,
    page: usize,
    page_size: usize,
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<TransactionSearchResult> {
    let match_expr = build_match_query(query);
    let has_match = !match_expr.is_empty();
    let has_filter = amount_min_cents.is_some()
        || amount_max_cents.is_some()
        || date_from.is_some()
        || date_to.is_some();
    // 空关键字 + 无筛选 → 空结果（既有语义）；空关键字 + 有筛选 → 仅筛选查询。
    if !has_match && !has_filter {
        return Ok(TransactionSearchResult {
            items: Vec::new(),
            total: 0,
            stale: reindex_queue_pending(conn)?,
        });
    }
    let page = page.max(1);
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
    // offset 用 saturating 运算 + try_from 钳制，防极端输入（usize::MAX）产生
    // debug 构建 panic 或 SQLite datatype mismatch（与 list_transactions 先例一致）
    let offset =
        i64::try_from(page.saturating_sub(1).saturating_mul(page_size)).unwrap_or(i64::MAX);

    // 动态拼 WHERE：基础软删除条件 + 可选的 MATCH / 金额 / 日期（含边界、单边可用）。
    // 参数顺序固定：MATCH → 金额下限 → 金额上限 → 起始日期 → 结束日期。
    let mut where_clauses: Vec<&str> = vec![
        "t.is_deleted = 0",
        "a.is_deleted = 0",
        "(c.is_deleted = 0 OR c.id IS NULL)",
    ];
    let mut params: Vec<rusqlite::types::Value> = Vec::new();
    if has_match {
        where_clauses.push("search_transactions MATCH ?");
        params.push(match_expr.into());
    }
    if let Some(min) = amount_min_cents {
        where_clauses.push("t.amount_cents >= ?");
        params.push(min.into());
    }
    if let Some(max) = amount_max_cents {
        where_clauses.push("t.amount_cents <= ?");
        params.push(max.into());
    }
    if let Some(from) = date_from {
        where_clauses.push("t.date >= ?");
        params.push(from.to_string().into());
    }
    if let Some(to) = date_to {
        where_clauses.push("t.date <= ?");
        params.push(to.to_string().into());
    }
    let where_sql = where_clauses.join(" AND ");

    // 有关键字时以 FTS MATCH 驱动；仅筛选（无关键字）时**不 JOIN FTS 虚拟表**，
    // 让金额 B-tree 索引（idx_transactions_amount，V006）与日期索引
    // （idx_transactions_date，V001）可被查询规划器选用（FTS 无约束 JOIN 会全扫）。
    let join = if has_match {
        format!(
            "FROM search_transactions s \
             JOIN transactions t ON s.transaction_id = t.id \
             JOIN accounts a ON t.account_id = a.id \
             LEFT JOIN categories c ON t.category_id = c.id \
             WHERE {where_sql}"
        )
    } else {
        format!(
            "FROM transactions t \
             JOIN accounts a ON t.account_id = a.id \
             LEFT JOIN categories c ON t.category_id = c.id \
             WHERE {where_sql}"
        )
    };

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) {join}"),
        rusqlite::params_from_iter(params.iter()),
        |r| r.get(0),
    )?;

    // 仅筛选（无 MATCH）时无 rank 列，排序退化为日期倒序 + id 兜底。
    let order_by = if has_match {
        "ORDER BY rank DESC, t.date DESC, t.created_at DESC, t.id DESC"
    } else {
        "ORDER BY t.date DESC, t.created_at DESC, t.id DESC"
    };
    let mut item_params = params;
    item_params.push((page_size as i64).into());
    item_params.push(offset.into());
    let items = query_all(
        conn,
        &format!(
            "SELECT t.id,t.kind,t.amount_cents,t.currency_code,t.amount_native_cents,t.account_id,\
             t.to_account_id,t.category_id,t.refund_of_transaction_id,t.note,t.date,t.created_at,\
             t.updated_at,t.version,t.device_id,t.is_deleted \
             {join} \
             {order_by} \
             LIMIT ? OFFSET ?"
        ),
        rusqlite::params_from_iter(item_params),
    )?;

    Ok(TransactionSearchResult {
        items,
        total,
        stale: reindex_queue_pending(conn)?,
    })
}
