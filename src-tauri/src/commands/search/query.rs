//! 查询执行：SQL 取候选（金额/日期过滤）→ Rust 按统一语义契约过滤 → 日期降序分页。
//!
//! 实现（issue #196）：弃用 FTS5（子序列语义与倒排索引根本不兼容），改为全量扫描。
//! SQL 层完成软删除口径过滤与可选金额/日期筛选并按交易日期降序预排序；Rust 层对
//! 候选逐条按统一语义契约（`text` 模块：原文连续子串 ∨ 拼音首字母子序列）过滤，
//! 过滤不改变顺序，随后内存分页。个人账本量级下全量扫描实测无感（10 万条约
//! 90ms/次、MB 级内存，见 issue #195）。

use rusqlite::Connection;

use crate::db::query::{FromRow, query_all};
use crate::error::Result;
use crate::models::{Transaction, TransactionSearchResult};

use super::text::{split_terms, term_matches};

/// 每页条数上限（防呆，防止极端输入拖垮查询）。
const MAX_PAGE_SIZE: usize = 200;

/// 候选行：交易行 + 转出账户名（可搜索字段之一，搜索时即时读取，改名即刻生效）。
struct Candidate {
    txn: Transaction,
    account_name: String,
}

impl FromRow for Candidate {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        // 列 0..=15 与 `Transaction::from_row` 的列清单一致，转出账户名追加在末列。
        Ok(Candidate {
            txn: Transaction::from_row(row)?,
            account_name: row.get(16)?,
        })
    }
}

/// 服务端分页搜索交易。词条之间 AND，每词条对备注/转出账户名按统一语义契约判定；
/// 排序固定交易日期降序（created_at、id 兜底，防同秒批量写入翻页漂移）；
/// 返回当前页与命中总数。
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
    let terms = split_terms(query);
    let has_filter = amount_min_cents.is_some()
        || amount_max_cents.is_some()
        || date_from.is_some()
        || date_to.is_some();
    // 空关键字 + 无筛选 → 空结果（既有语义）；空关键字 + 有筛选 → 仅筛选查询。
    if terms.is_empty() && !has_filter {
        return Ok(TransactionSearchResult {
            items: Vec::new(),
            total: 0,
        });
    }
    let page = page.max(1);
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE);

    // SQL 层：软删除口径（交易、账户、分类，与交易列表一致，含黑洞账户）+
    // 可选金额/日期过滤（走既有 B-tree 索引），并按日期降序预排序。
    let mut where_clauses: Vec<&str> = vec![
        "t.is_deleted = 0",
        "a.is_deleted = 0",
        "(c.is_deleted = 0 OR c.id IS NULL)",
    ];
    let mut params: Vec<rusqlite::types::Value> = Vec::new();
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
    let candidates: Vec<Candidate> = query_all(
        conn,
        &format!(
            "SELECT t.id,t.kind,t.amount_cents,t.currency_code,t.amount_native_cents,t.account_id,\
             t.to_account_id,t.category_id,t.refund_of_transaction_id,t.note,t.date,t.created_at,\
             t.updated_at,t.version,t.device_id,t.is_deleted,COALESCE(a.name,'') \
             FROM transactions t \
             JOIN accounts a ON t.account_id = a.id \
             LEFT JOIN categories c ON t.category_id = c.id \
             WHERE {} \
             ORDER BY t.date DESC, t.created_at DESC, t.id DESC",
            where_clauses.join(" AND ")
        ),
        rusqlite::params_from_iter(params.iter()),
    )?;

    // Rust 层：统一语义过滤（词条之间 AND；每词条对备注/转出账户名任一命中即算）。
    // 过滤保持 SQL 给定序（日期降序），不重排。
    let matched: Vec<Transaction> = candidates
        .into_iter()
        .filter(|c| {
            terms
                .iter()
                .all(|t| term_matches(t, c.txn.note.as_deref(), &c.account_name))
        })
        .map(|c| c.txn)
        .collect();

    let total = matched.len() as i64;
    // saturating 运算防极端输入（usize::MAX）下溢/溢出 panic（与 list_transactions 先例一致）；
    // 超出命中数的页返回空页。
    let offset = page.saturating_sub(1).saturating_mul(page_size);
    let items = matched.into_iter().skip(offset).take(page_size).collect();

    Ok(TransactionSearchResult { items, total })
}
