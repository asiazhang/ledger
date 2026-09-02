//! 查询执行：SQL 取候选（金额/日期过滤）→ Rust 按统一语义契约过滤 → 日期降序分页。
//!
//! 实现（issue #196）：子序列匹配语义与任何倒排索引都不兼容，改为全量扫描。
//! SQL 层完成软删除口径过滤与可选金额/日期筛选并按交易日期降序预排序；Rust 层对
//! 候选逐条按统一语义契约（`text` 模块：原文连续子串 ∨ 拼音首字母子序列）过滤，
//! 过滤不改变顺序，随后流式分页（ADR-0043：游标逐行、仅当前页物化，内存 O(1)）。
//! 个人账本量级下全量扫描实测无感（10 万条约 90ms/次，见 issue #195）。

use rusqlite::Connection;

use crate::db::query::FromRow;
use crate::error::Result;
use crate::models::{Transaction, TransactionSearchResult};

use crate::transaction::search_text::{split_terms, term_matches};

/// 每页条数上限（防呆，防止极端输入拖垮查询）。
const MAX_PAGE_SIZE: usize = 200;

/// 候选行：交易行 + 转出账户名 + 商户名（可搜索字段，搜索时即时读取，
/// 账户/商户改名即刻生效）。商户不按软删过滤：软删商户的历史交易仍可搜。
struct Candidate {
    txn: Transaction,
    account_name: String,
    merchant_name: Option<String>,
}

impl FromRow for Candidate {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        // 列 0..=17 与 `Transaction::from_row` 的列清单一致（末列为 policy_id），
        // 转出账户名、商户名追加在末两列。
        Ok(Candidate {
            txn: Transaction::from_row(row)?,
            account_name: row.get(18)?,
            merchant_name: row.get(19)?,
        })
    }
}

/// 服务端分页搜索交易。词条之间 AND，每词条对备注/转出账户名/商户名按统一语义契约判定；
/// 排序固定交易日期降序（created_at、id 兜底，防同秒批量写入翻页漂移）；
/// 返回当前页与命中总数。
///
/// 支持可选筛选（与关键字 AND 组合，全部可省略、单边可用）：
/// - `amount_min_cents` / `amount_max_cents`：金额区间（整数分，含边界；按本位币分
///   `amount_native_cents` 过滤，与全仓聚合口径同源，多币种下跨币种不再混滤）；
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
        // 本位币分口径（issue #395）：与全仓聚合一致，多币种下跨币种不再混滤。
        where_clauses.push("t.amount_native_cents >= ?");
        params.push(min.into());
    }
    if let Some(max) = amount_max_cents {
        where_clauses.push("t.amount_native_cents <= ?");
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
    // SQL 层取候选后流式处理（ADR-0043）：游标逐行读取 → 逐行按统一语义契约过滤
    // （词条之间 AND；每词条对备注/转出账户名/商户名任一命中即算）→ 命中计数 total
    // → 仅当前页条目物化，候选与命中均不整体驻留（内存 O(1)）。过滤保持 SQL 给定序
    // （日期降序），不重排。拼音首字母按候选×词条惰性重算，不预计算：个人账本量级
    // 实测远低于感知阈值（ADR-0027，10 万条约 90ms/次）。
    let mut stmt = conn.prepare(&format!(
        "SELECT t.id,t.kind,t.amount_cents,t.currency_code,t.amount_native_cents,t.account_id,\
         t.to_account_id,t.category_id,t.refund_of_transaction_id,t.note,t.date,t.created_at,\
         t.updated_at,t.version,t.device_id,t.is_deleted,t.merchant_id,t.policy_id,COALESCE(a.name,''),m.name \
         FROM transactions t \
         JOIN accounts a ON t.account_id = a.id \
         LEFT JOIN categories c ON t.category_id = c.id \
         LEFT JOIN merchants m ON t.merchant_id = m.id \
         WHERE {} \
         ORDER BY t.date DESC, t.created_at DESC, t.id DESC",
        where_clauses.join(" AND ")
    ))?;
    // saturating 运算防极端输入（usize::MAX）下溢/溢出 panic（与 list_transactions 先例一致）；
    // 超出命中数的页返回空页。
    let offset = page.saturating_sub(1).saturating_mul(page_size);
    let mut total: i64 = 0;
    let mut items: Vec<Transaction> = Vec::with_capacity(page_size);
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        Candidate::from_row,
    )?;
    for row in rows {
        let c = row?;
        if terms.iter().all(|t| {
            term_matches(
                t,
                c.txn.note.as_deref(),
                &c.account_name,
                c.merchant_name.as_deref(),
            )
        }) {
            total += 1;
            // 命中序号（0 起）落在当前页区间且未满页才物化。
            if total as usize > offset && items.len() < page_size {
                items.push(c.txn);
            }
        }
    }

    Ok(TransactionSearchResult { items, total })
}
