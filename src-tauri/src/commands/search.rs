//! 交易搜索（ADR-0004）：FTS5 索引维护与查询。
//!
//! - 可搜索内容：备注 + 账户名 + 二者拼音首字母（仅首字母缩写、小写）。
//! - 匹配语义：整词匹配 + 拼音首字母匹配 + 前缀通配；词条间 AND、词条内原词/前缀 OR。
//! - 索引维护（ADR-0004 决策 #14 刷新策略）：**后台定时刷新**——交易写入路径不做任何
//!   同步索引工作（界面操作零索引开销），由触发器纯 SQL 入队 `search_reindex_queue`，
//!   后台线程固定周期（默认 60s）消费队列批量重建；批量导入完成后在命令内立即消费一次；
//!   启动时按文档数对账兜底全量重建。
//! - 搜索结果附 `stale` 标志：队列非空（存在未消费写入）时 true，供前端提示索引可能滞后。

use rusqlite::Connection;
use rusqlite::OptionalExtension;
use std::sync::{Arc, Mutex};
use tauri::State;

use crate::db::DbState;
use crate::db::query::query_all;
use crate::error::Result;
use crate::models::TransactionSearchResult;

// ---------------------------------------------------------------------------
// 拼音首字母与可搜索内容
// ---------------------------------------------------------------------------

/// 常见多音字在记账语境下的读音修正（前字 + 当前字 → 拼音首字母）。
/// `pinyin` crate 按单字常用读音取音，无上下文消歧；此处用简单前字规则覆盖
/// 高频金融/账户场景的例外读音，其余多音字沿用默认读音（已知局限）。
fn polyphone_initial(prev: Option<char>, ch: char) -> Option<char> {
    match ch {
        // 行：银行/商业银行等 → háng（h）；默认行走/行为 → xíng（x）
        '行' if prev == Some('银') => Some('h'),
        _ => None,
    }
}

/// 生成拼音首字母缩写（小写）。逐字符处理：
/// - 中文字符取拼音（无声调）首字母，如「招商银行」→ `zsyh`（银行 → yh，多音字修正）；
/// - ASCII 字母/数字小写保留（如 `ABC银行` → `abcyh`，`123` → `123`）；
/// - 其余字符（标点、空格等）跳过。
pub fn pinyin_initials(text: &str) -> String {
    use pinyin::ToPinyin;
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<char> = None;
    for ch in text.chars() {
        if let Some(first) = polyphone_initial(prev, ch) {
            out.push(first);
        } else if let Some(py) = ch.to_pinyin() {
            if let Some(first) = py.plain().chars().next() {
                out.push(first.to_ascii_lowercase());
            }
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        }
        prev = Some(ch);
    }
    out
}

/// 拼接可搜索内容：`备注 账户名 备注拼音 账户名拼音`。
/// 空字段跳过；所有字段为空时返回空串（仍保留文档行）。
pub fn build_search_content(note: Option<&str>, account_name: &str) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(4);
    let text_parts = [note, Some(account_name)];
    for text in text_parts.into_iter().flatten() {
        let text = text.trim();
        if !text.is_empty() {
            parts.push(text.to_string());
        }
    }
    for text in text_parts.into_iter().flatten() {
        let initials = pinyin_initials(text);
        if !initials.is_empty() {
            parts.push(initials);
        }
    }
    parts.join(" ")
}

// ---------------------------------------------------------------------------
// 查询构建
// ---------------------------------------------------------------------------

/// FTS5 查询中需引号包裹才视为字面量的字符（除 `"` 与 `*` 另有处理外，
/// 引号包裹已覆盖 AND/OR/NOT/NEAR/括号/连字符/冒号/脱字符/加号等全部特殊语法）。
/// `"` 在 FTS5 短语中无法转义（实测 `""` 双写不支持），直接剥离；
/// `*` 剥离以避免用户手输通配符干扰（前缀通配由本函数统一附加）。
///
/// 按空白分词；每个词条生成 `"词条"` 与 `"词条"*`（前缀通配）两个变体并 OR，
/// 词条之间 AND 连接。如 `cf 午餐` → `("cf" OR "cf"*) AND ("午餐" OR "午餐"*)`。
/// 空查询返回空串，调用方应直接返回空结果。
pub fn build_match_query(query: &str) -> String {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| {
            t.chars()
                .filter(|&c| c != '"' && c != '*')
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .collect();
    if terms.is_empty() {
        return String::new();
    }
    terms
        .iter()
        .map(|t| format!("(\"{t}\" OR \"{t}\"*)"))
        .collect::<Vec<_>>()
        .join(" AND ")
}

// ---------------------------------------------------------------------------
// 查询执行
// ---------------------------------------------------------------------------

/// 每页条数上限（防呆，防止极端输入拖垮查询）。
const MAX_PAGE_SIZE: usize = 200;

/// 后台刷新周期：固定间隔轮询搜索重建队列（秒）。
/// 时效性要求低（用户可接受分钟级滞后），周期内写入不立即可搜。
const REFRESH_INTERVAL_SECS: u64 = 60;

/// 搜索重建队列是否非空（存在尚未消费的写入）。
fn reindex_queue_pending(conn: &Connection) -> Result<bool> {
    let pending: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM search_reindex_queue)",
        [],
        |r| r.get(0),
    )?;
    Ok(pending != 0)
}

/// 启动后台索引刷新线程：固定周期（`REFRESH_INTERVAL_SECS`）检查搜索重建队列，
/// 非空则消费批量重建 FTS 文档。空队列时仅做一次存在性查询，开销可忽略。
/// Daemon 线程随进程退出，无需优雅停止（Tauri 退出即进程结束）。
/// 每次周期短暂持 `Mutex<Connection>`，消费小批量、持锁毫秒级；
/// 全量重建仅在启动对账（`reconcile_search_index`）时执行，不在此线程内。
pub fn start_search_refresh_thread(conn: Arc<Mutex<Connection>>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(REFRESH_INTERVAL_SECS));
            let Ok(guard) = conn.lock() else {
                // 连接锁损坏（极不可能）：跳过本轮，下个周期重试。
                continue;
            };
            let _ = process_reindex_queue(&guard);
        }
    });
}

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

/// IPC 命令：搜索交易（可选金额/日期筛选与关键字 AND 组合）。
/// 四个筛选参数与内部函数一一对应（issue #40），作为独立命令参数暴露，
/// 前端按 issue #41 契约以 camelCase 键名调用（Tauri 自动转 snake_case），
/// 故显式 allow `too_many_arguments`。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn search_transactions(
    db: State<'_, DbState>,
    query: String,
    page: Option<usize>,
    page_size: Option<usize>,
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Result<TransactionSearchResult> {
    let conn = db
        .conn
        .lock()
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    search_transactions_internal(
        &conn,
        &query,
        page.unwrap_or(1),
        page_size.unwrap_or(20),
        amount_min_cents,
        amount_max_cents,
        date_from.as_deref(),
        date_to.as_deref(),
    )
}

// ---------------------------------------------------------------------------
// 索引维护
// ---------------------------------------------------------------------------

/// 重建单条交易的 FTS 文档（读取当前内容组装后 upsert）。
/// 交易不存在或已软删除时删除对应文档。
pub fn reindex_transaction(conn: &Connection, transaction_id: &str) -> Result<()> {
    let Some(payload) = read_index_payload(conn, transaction_id)? else {
        return delete_index_document(conn, transaction_id);
    };
    if payload.is_deleted {
        return delete_index_document(conn, transaction_id);
    }
    upsert_index_document(conn, transaction_id, &payload.content)
}

/// 索引载荷：可搜索内容与软删除标志。
struct IndexPayload {
    content: String,
    is_deleted: bool,
}

/// 读取交易的索引载荷。交易不存在返回 None。
/// 内容 = 备注 + 账户名 + 二者拼音首字母。
fn read_index_payload(conn: &Connection, transaction_id: &str) -> Result<Option<IndexPayload>> {
    let row = conn
        .query_row(
            "SELECT t.note, COALESCE(a.name,''), t.is_deleted \
             FROM transactions t \
             LEFT JOIN accounts a ON t.account_id = a.id \
             WHERE t.id = ?1",
            [transaction_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(row.map(|(note, account_name, is_deleted)| IndexPayload {
        content: build_search_content(note.as_deref(), &account_name),
        is_deleted: is_deleted != 0,
    }))
}

/// 删除单条交易的 FTS 文档（不存在时为空操作）。
pub fn delete_index_document(conn: &Connection, transaction_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM search_transactions WHERE transaction_id = ?1",
        [transaction_id],
    )?;
    Ok(())
}

/// 消费搜索重建队列：按入队时间升序逐条重建 FTS 文档并删除队列行，返回处理条数。
/// 账户/分类改名、绕过应用层的写入产生的待办由此收敛。
pub fn process_reindex_queue(conn: &Connection) -> Result<usize> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT transaction_id FROM search_reindex_queue \
             ORDER BY enqueued_at ASC, transaction_id ASC",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    let mut processed = 0;
    for id in &ids {
        reindex_transaction(conn, id)?;
        conn.execute(
            "DELETE FROM search_reindex_queue WHERE transaction_id = ?1",
            [id],
        )?;
        processed += 1;
    }
    Ok(processed)
}

/// 全量重建搜索索引：清空全部 FTS 文档后为所有未删除交易重建，并清空重建队列。
/// 幂等，供迁移后存量数据一次性建索引与启动对账兜底使用。
pub fn rebuild_search_index(conn: &Connection) -> Result<usize> {
    conn.execute("DELETE FROM search_transactions", [])?;
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM transactions WHERE is_deleted=0 ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    let mut count = 0;
    for id in &ids {
        reindex_transaction(conn, id)?;
        count += 1;
    }
    conn.execute("DELETE FROM search_reindex_queue", [])?;
    Ok(count)
}

/// 启动对账：FTS 文档数 ≠ 未删除交易数 → 全量重建（覆盖迁移前存量与漏建文档）；
/// 一致 → 消费重建队列。
pub fn reconcile_search_index(conn: &Connection) -> Result<()> {
    let fts_count: i64 =
        conn.query_row("SELECT count(*) FROM search_transactions", [], |r| r.get(0))?;
    let live_count: i64 = conn.query_row(
        "SELECT count(*) FROM transactions WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )?;
    if fts_count != live_count {
        rebuild_search_index(conn)?;
    } else {
        process_reindex_queue(conn)?;
    }
    Ok(())
}

/// 查询某交易的 FTS 文档 rowid（contentful 表可直接按列删除，rowid 仅用于原地 REPLACE）。
fn find_doc_rowid(conn: &Connection, transaction_id: &str) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT rowid FROM search_transactions WHERE transaction_id = ?1",
        [transaction_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(Into::into)
}

/// upsert 一条 FTS 文档：已有文档按原 rowid `INSERT OR REPLACE`（词条干净替换，
/// 不残留旧内容）；新文档不指定 rowid 由 FTS5 自动分配。
fn upsert_index_document(conn: &Connection, transaction_id: &str, content: &str) -> Result<()> {
    match find_doc_rowid(conn, transaction_id)? {
        Some(rowid) => {
            conn.execute(
                "INSERT OR REPLACE INTO search_transactions(rowid, content, transaction_id) \
                 VALUES(?1, ?2, ?3)",
                rusqlite::params![rowid, content, transaction_id],
            )?;
        }
        None => {
            conn.execute(
                "INSERT INTO search_transactions(content, transaction_id) VALUES(?1, ?2)",
                rusqlite::params![content, transaction_id],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{init_db, open_in_memory};

    fn setup() -> Connection {
        let mut conn = open_in_memory().unwrap();
        init_db(&mut conn).unwrap();
        conn
    }

    /// 无筛选搜索（第 1 页、每页 20 条）。
    fn search(conn: &Connection, query: &str) -> Result<TransactionSearchResult> {
        search_transactions_internal(conn, query, 1, 20, None, None, None, None)
    }

    /// 无筛选分页搜索。
    fn search_paged(
        conn: &Connection,
        query: &str,
        page: usize,
        page_size: usize,
    ) -> Result<TransactionSearchResult> {
        search_transactions_internal(conn, query, page, page_size, None, None, None, None)
    }

    fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            rusqlite::params![id, name, kind, currency],
        )
        .unwrap();
    }

    fn insert_category(conn: &Connection, id: &str, name: &str, kind: &str) {
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,NULL,NULL,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            rusqlite::params![id, name, kind],
        )
        .unwrap();
    }

    fn insert_txn(
        conn: &Connection,
        id: &str,
        account_id: &str,
        category_id: Option<&str>,
        note: Option<&str>,
        date: &str,
    ) {
        conn.execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
             category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'expense',1000,'CNY',1000,?2,NULL,?3,NULL,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            rusqlite::params![id, account_id, category_id, note, date],
        )
        .unwrap();
    }

    /// 指定金额的存量交易（其余列与 `insert_txn` 一致）。
    fn insert_txn_amount(
        conn: &Connection,
        id: &str,
        account_id: &str,
        note: Option<&str>,
        date: &str,
        amount_cents: i64,
    ) {
        conn.execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
             category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'expense',?2,'CNY',?2,?3,NULL,NULL,NULL,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            rusqlite::params![id, amount_cents, account_id, note, date],
        )
        .unwrap();
    }

    // -----------------------------------------------------------------------
    // 拼音首字母
    // -----------------------------------------------------------------------

    #[test]
    fn pinyin_initials_basic() {
        assert_eq!(pinyin_initials("招商银行"), "zsyh");
        assert_eq!(pinyin_initials("吃饭"), "cf");
        assert_eq!(pinyin_initials("餐饮"), "cy");
        assert_eq!(pinyin_initials("工资"), "gz");
    }

    #[test]
    fn pinyin_initials_handles_mixed_and_ascii() {
        assert_eq!(pinyin_initials("ABC银行"), "abcyh");
        assert_eq!(pinyin_initials("无(CNY)"), "wcny");
        assert_eq!(pinyin_initials("12306"), "12306");
        assert_eq!(pinyin_initials(""), "");
        assert_eq!(pinyin_initials("---"), "");
    }

    #[test]
    fn pinyin_initials_all_lowercase() {
        let out = pinyin_initials("招商银行");
        assert_eq!(out, out.to_lowercase());
    }

    // -----------------------------------------------------------------------
    // 可搜索内容组装
    // -----------------------------------------------------------------------

    #[test]
    fn build_content_joins_note_account_and_initials() {
        let content = build_search_content(Some("吃饭"), "招商银行");
        assert_eq!(content, "吃饭 招商银行 cf zsyh");
    }

    #[test]
    fn build_content_skips_empty_fields() {
        assert_eq!(build_search_content(None, "现金"), "现金 xj");
        assert_eq!(build_search_content(Some("   "), "现金"), "现金 xj");
        assert_eq!(build_search_content(None, ""), "");
    }

    // -----------------------------------------------------------------------
    // 查询构建
    // -----------------------------------------------------------------------

    #[test]
    fn build_match_query_single_term_with_prefix() {
        assert_eq!(build_match_query("午餐"), "(\"午餐\" OR \"午餐\"*)");
        assert_eq!(build_match_query("cf"), "(\"cf\" OR \"cf\"*)");
    }

    #[test]
    fn build_match_query_multi_terms_and_joined() {
        assert_eq!(
            build_match_query("cf 午餐"),
            "(\"cf\" OR \"cf\"*) AND (\"午餐\" OR \"午餐\"*)"
        );
    }

    #[test]
    fn build_match_query_escapes_special_chars() {
        // AND/OR/NOT 被引号包裹后成为字面量
        assert_eq!(
            build_match_query("午餐 AND 晚餐"),
            "(\"午餐\" OR \"午餐\"*) AND (\"AND\" OR \"AND\"*) AND (\"晚餐\" OR \"晚餐\"*)"
        );
        // 引号与星号剥离
        assert_eq!(build_match_query("a\"b*c"), "(\"abc\" OR \"abc\"*)");
        // 括号等保留在引号内
        assert_eq!(build_match_query("(abc)"), "(\"(abc)\" OR \"(abc)\"*)");
    }

    #[test]
    fn build_match_query_empty_and_whitespace() {
        assert_eq!(build_match_query(""), "");
        assert_eq!(build_match_query("   "), "");
        assert_eq!(build_match_query("\"\"\"\"**"), "");
    }

    // -----------------------------------------------------------------------
    // 搜索行为
    // -----------------------------------------------------------------------

    #[test]
    fn search_matches_note_account_and_pinyin() {
        let conn = setup();
        insert_account(&conn, "acc-1", "招商银行", "bank", "CNY");
        insert_account(&conn, "acc-2", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("吃饭"), "2026-02-01");
        insert_txn(&conn, "tx-2", "acc-2", None, None, "2026-02-02");
        rebuild_search_index(&conn).unwrap();

        // 备注整词
        let r = search(&conn, "吃饭").unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.items[0].id, "tx-1");
        // 拼音首字母 cf
        assert_eq!(search(&conn, "cf").unwrap().total, 1);
        // 账户名整词
        assert_eq!(search(&conn, "招商").unwrap().total, 1);
        // 账户名拼音 zsyh
        assert_eq!(search(&conn, "zsyh").unwrap().total, 1);
        // 前缀通配：吃 → 吃饭
        assert_eq!(search(&conn, "吃").unwrap().total, 1);
        // 整词不命中子串
        assert_eq!(search(&conn, "商银").unwrap().total, 0);
    }

    #[test]
    fn search_multi_keyword_and_combination() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        insert_txn(&conn, "tx-2", "acc-1", None, Some("晚餐"), "2026-02-02");
        rebuild_search_index(&conn).unwrap();

        // 两个词条同时命中才返回（AND 语义）
        let r = search(&conn, "午餐 现金").unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.items[0].id, "tx-1");
        let r = search(&conn, "午餐 晚餐").unwrap();
        assert_eq!(r.total, 0);
    }

    #[test]
    fn search_excludes_soft_deleted() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        rebuild_search_index(&conn).unwrap();
        assert_eq!(search(&conn, "午餐").unwrap().total, 1);

        // 软删除后索引文档被移除，搜索结果消失
        conn.execute(
            "UPDATE transactions SET is_deleted=1, updated_at='2026-02-03T00:00:00Z', version=version+1 WHERE id='tx-1'",
            [],
        )
        .unwrap();
        process_reindex_queue(&conn).unwrap();
        assert_eq!(search(&conn, "午餐").unwrap().total, 0);
    }

    #[test]
    fn search_rank_first_then_date_desc() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        // tx-a：命中词条更多、相关度更高，但日期更早
        insert_txn(
            &conn,
            "tx-a",
            "acc-1",
            None,
            Some("午餐 晚餐 早餐"),
            "2026-01-01",
        );
        // tx-b：命中词条更少、相关度更低，但日期更新
        insert_txn(&conn, "tx-b", "acc-1", None, Some("午餐"), "2026-02-01");
        rebuild_search_index(&conn).unwrap();

        let r = search(&conn, "午餐").unwrap();
        assert_eq!(r.items[0].id, "tx-a", "相关度 rank 优先于日期倒序");
        assert_eq!(r.items[1].id, "tx-b");
    }

    #[test]
    fn search_pagination_and_total() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        for i in 1..=5 {
            insert_txn(
                &conn,
                &format!("tx-{i}"),
                "acc-1",
                None,
                Some("午餐"),
                &format!("2026-01-{i:02}"),
            );
        }
        rebuild_search_index(&conn).unwrap();

        let r = search_paged(&conn, "午餐", 1, 2).unwrap();
        assert_eq!(r.total, 5);
        assert_eq!(r.items.len(), 2);
        assert_eq!(r.items[0].id, "tx-5", "日期倒序第一页首条应为最新");

        let r = search_paged(&conn, "午餐", 3, 2).unwrap();
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].id, "tx-1");
    }

    #[test]
    fn search_transfer_by_account_name() {
        use crate::commands::transactions::insert_transaction;
        use crate::models::TransactionInput;
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_account(&conn, "acc-2", "招商银行", "bank", "CNY");
        let input = TransactionInput {
            kind: "transfer".into(),
            amount_cents: 3000,
            currency_code: "CNY".into(),
            account_id: "acc-1".into(),
            to_account_id: Some("acc-2".into()),
            category_id: None,
            refund_of_transaction_id: None,
            note: Some("转账".into()),
            date: "2026-02-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        };
        let id = insert_transaction(&conn, input).unwrap();
        // 写入路径不做同步索引（ADR-0004 决策 #14）：消费队列后转出账户名
        // （含拼音首字母）可搜；转入账户名不在索引中
        process_reindex_queue(&conn).unwrap();
        assert_eq!(search(&conn, "现金").unwrap().total, 1);
        assert_eq!(search(&conn, "xj").unwrap().total, 1);
        assert_eq!(search(&conn, "招商").unwrap().total, 0);
        let _ = id;
    }

    #[test]
    fn search_extreme_page_inputs_do_not_panic() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        rebuild_search_index(&conn).unwrap();

        // 极端输入：usize::MAX 页/页大小不 panic、不破坏 total；page=0 钳制为 1
        let r = search_paged(&conn, "午餐", usize::MAX, usize::MAX).unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.items.len(), 0);
        let r = search_paged(&conn, "午餐", 0, 0).unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.items.len(), 1);
    }

    #[test]
    fn search_empty_query_and_special_chars() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        rebuild_search_index(&conn).unwrap();

        assert_eq!(search(&conn, "").unwrap().total, 0);
        assert_eq!(search(&conn, "   ").unwrap().total, 0);
        // 特殊字符不报错、不误命中
        let r = search(&conn, "午餐 AND 现金 OR (NOT)").unwrap();
        assert_eq!(r.total, 0);
        let r = search(&conn, "午餐\"").unwrap();
        assert_eq!(r.total, 1, "剥离引号后仍命中");
    }

    // -----------------------------------------------------------------------
    // 金额/日期筛选（issue #40）
    // -----------------------------------------------------------------------

    #[test]
    fn search_amount_range_inclusive_bounds() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1550);
        insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 2000);
        insert_txn_amount(&conn, "tx-3", "acc-1", None, "2026-02-03", 3000);
        rebuild_search_index(&conn).unwrap();

        // 区间含边界：1550 与 2000 都应命中，3000 不命中
        let r = search_transactions_internal(&conn, "", 1, 20, Some(1550), Some(2000), None, None)
            .unwrap();
        assert_eq!(r.total, 2);
        assert_eq!(r.items[0].id, "tx-2", "无关键字时按日期倒序");
        assert_eq!(r.items[1].id, "tx-1");
    }

    #[test]
    fn search_amount_filter_one_sided() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1000);
        insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 1500);
        insert_txn_amount(&conn, "tx-3", "acc-1", None, "2026-02-03", 2000);
        rebuild_search_index(&conn).unwrap();

        // 只填下限（含边界）
        let r =
            search_transactions_internal(&conn, "", 1, 20, Some(1500), None, None, None).unwrap();
        assert_eq!(r.total, 2, "金额下限含边界：1500、2000");
        // 只填上限（含边界）
        let r =
            search_transactions_internal(&conn, "", 1, 20, None, Some(1500), None, None).unwrap();
        assert_eq!(r.total, 2, "金额上限含边界：1000、1500");
    }

    #[test]
    fn search_date_range_inclusive_bounds() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1000);
        insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-05", 1000);
        insert_txn_amount(&conn, "tx-3", "acc-1", None, "2026-02-10", 1000);
        rebuild_search_index(&conn).unwrap();

        // 日期区间含边界：02-01 与 02-05 命中，02-10 不命中
        let r = search_transactions_internal(
            &conn,
            "",
            1,
            20,
            None,
            None,
            Some("2026-02-01"),
            Some("2026-02-05"),
        )
        .unwrap();
        assert_eq!(r.total, 2);
        // 单边日期
        let r =
            search_transactions_internal(&conn, "", 1, 20, None, None, Some("2026-02-05"), None)
                .unwrap();
        assert_eq!(r.total, 2, "起始日期含边界：02-05、02-10");
        let r =
            search_transactions_internal(&conn, "", 1, 20, None, None, None, Some("2026-02-05"))
                .unwrap();
        assert_eq!(r.total, 2, "结束日期含边界：02-01、02-05");
    }

    #[test]
    fn search_filters_combined_with_keyword_and() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        // 命中关键字 + 金额区间 + 日期区间
        insert_txn_amount(&conn, "tx-1", "acc-1", Some("午餐"), "2026-02-01", 1550);
        // 金额超区间
        insert_txn_amount(&conn, "tx-2", "acc-1", Some("午餐"), "2026-02-02", 3000);
        // 日期超区间
        insert_txn_amount(&conn, "tx-3", "acc-1", Some("午餐"), "2026-02-10", 1550);
        // 金额、日期均命中但无关键字
        insert_txn_amount(&conn, "tx-4", "acc-1", None, "2026-02-03", 1550);
        rebuild_search_index(&conn).unwrap();

        let r = search_transactions_internal(
            &conn,
            "午餐",
            1,
            20,
            Some(1550),
            Some(2000),
            Some("2026-02-01"),
            Some("2026-02-05"),
        )
        .unwrap();
        assert_eq!(r.total, 1, "关键字与金额/日期筛选 AND 组合");
        assert_eq!(r.items[0].id, "tx-1");
    }

    #[test]
    fn search_filters_only_without_keyword() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn_amount(&conn, "tx-1", "acc-1", Some("午餐"), "2026-02-01", 1550);
        insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 3000);
        rebuild_search_index(&conn).unwrap();

        // 空查询 + 有筛选 → 执行仅筛选查询（放开空查询直返空）
        let r = search_transactions_internal(&conn, "   ", 1, 20, Some(2000), None, None, None)
            .unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.items[0].id, "tx-2");
        // 空查询 + 无筛选 → 维持空结果
        assert_eq!(search(&conn, "").unwrap().total, 0);
        assert_eq!(search(&conn, "   ").unwrap().total, 0);
    }

    // -----------------------------------------------------------------------
    // 后台定时刷新（ADR-0004 决策 #14）与 stale 标志
    // -----------------------------------------------------------------------

    #[test]
    fn write_path_does_not_index_until_queue_consumed() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        // 单笔写入路径不再同步建索引（触发器已入队）：未消费前搜不到
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        assert_eq!(
            search(&conn, "午餐").unwrap().total,
            0,
            "写入后未刷新不可搜"
        );
        // 消费队列后立即可搜
        process_reindex_queue(&conn).unwrap();
        assert_eq!(search(&conn, "午餐").unwrap().total, 1);
    }

    #[test]
    fn search_reports_stale_while_queue_pending() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        rebuild_search_index(&conn).unwrap();
        assert!(!search(&conn, "午餐").unwrap().stale, "队列为空时不滞后");

        // 软删除入队（触发器）后队列非空：搜索报告 stale=true（搜索不消费队列）
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        let r = search(&conn, "午餐").unwrap();
        assert!(r.stale, "存在未消费写入时 stale=true");

        // 消费后队列清空：stale 回落 false
        process_reindex_queue(&conn).unwrap();
        assert!(!search(&conn, "午餐").unwrap().stale);
    }

    #[test]
    fn batch_import_consumes_queue_immediately() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        let input = crate::models::TransactionInput {
            kind: "expense".into(),
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: "acc-1".into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: Some("午餐".into()),
            date: "2026-02-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        };
        // 批量导入命令内部在事务提交后立即消费队列
        crate::commands::transactions::create_transactions_internal(&conn, vec![input], false)
            .unwrap();
        assert_eq!(search(&conn, "午餐").unwrap().total, 1, "导入后立即可搜");
        assert!(!search(&conn, "午餐").unwrap().stale, "导入消费后不滞后");
    }

    #[test]
    fn search_amount_and_date_filters_without_keyword() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1000);
        insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 1550);
        insert_txn_amount(&conn, "tx-3", "acc-1", None, "2026-02-10", 1550);
        rebuild_search_index(&conn).unwrap();

        // 无关键字 + 金额与日期同时筛选（AND 组合，含边界）
        let r = search_transactions_internal(
            &conn,
            "",
            1,
            20,
            Some(1500),
            Some(2000),
            Some("2026-02-01"),
            Some("2026-02-05"),
        )
        .unwrap();
        assert_eq!(r.total, 1, "金额与日期同时命中才返回");
        assert_eq!(r.items[0].id, "tx-2");
    }

    #[test]
    fn search_filters_exclude_soft_deleted() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn_amount(&conn, "tx-1", "acc-1", None, "2026-02-01", 1550);
        insert_txn_amount(&conn, "tx-2", "acc-1", None, "2026-02-02", 1550);
        rebuild_search_index(&conn).unwrap();

        conn.execute(
            "UPDATE transactions SET is_deleted=1, updated_at='2026-02-03T00:00:00Z', version=version+1 WHERE id='tx-2'",
            [],
        )
        .unwrap();
        let r = search_transactions_internal(&conn, "", 1, 20, Some(1550), Some(1550), None, None)
            .unwrap();
        assert_eq!(r.total, 1, "仅筛选查询同样排除软删除");
        assert_eq!(r.items[0].id, "tx-1");
    }

    #[test]
    fn search_includes_hidden_account_and_all_kinds() {
        let conn = setup();
        // 黑洞账户（种子 无(CNY) 已存在）：income 入黑洞账户
        let hidden_id: String = conn
            .query_row(
                "SELECT id FROM accounts WHERE is_hidden=1 AND currency_code='CNY'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        insert_txn(
            &conn,
            "tx-hidden",
            &hidden_id,
            None,
            Some("退款入账"),
            "2026-02-01",
        );

        rebuild_search_index(&conn).unwrap();
        let r = search(&conn, "退款").unwrap();
        assert_eq!(r.total, 1, "黑洞账户交易可搜");
        assert_eq!(r.items[0].id, "tx-hidden");
    }

    #[test]
    fn rebuild_is_idempotent_and_covers_legacy_data() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        // 存量数据：直接 SQL 插入（模拟 V005 迁移前），不调用应用层重建
        let n1 = rebuild_search_index(&conn).unwrap();
        assert_eq!(n1, 1);
        // 幂等：重复重建结果一致
        let n2 = rebuild_search_index(&conn).unwrap();
        assert_eq!(n2, 1);
        assert_eq!(search(&conn, "午餐").unwrap().total, 1);
    }

    #[test]
    fn reconcile_rebuilds_when_counts_mismatch() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        // FTS 为空（存量），counts 不匹配 → 全量重建
        reconcile_search_index(&conn).unwrap();
        assert_eq!(search(&conn, "午餐").unwrap().total, 1);
        // 再次对账：一致 → 走队列消费，结果不变
        reconcile_search_index(&conn).unwrap();
        assert_eq!(search(&conn, "午餐").unwrap().total, 1);
    }

    #[test]
    fn account_rename_updates_searchable_content() {
        let conn = setup();
        insert_account(&conn, "acc-1", "招商银行", "bank", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, None, "2026-02-01");
        rebuild_search_index(&conn).unwrap();
        assert_eq!(search(&conn, "招商").unwrap().total, 1);

        // 账户改名：触发器入队，消费后新名称生效
        conn.execute(
            "UPDATE accounts SET name='民生银行', updated_at='2026-02-02T00:00:00Z', version=version+1 WHERE id='acc-1'",
            [],
        )
        .unwrap();
        process_reindex_queue(&conn).unwrap();
        assert_eq!(search(&conn, "招商").unwrap().total, 0);
        assert_eq!(search(&conn, "民生").unwrap().total, 1);
        assert_eq!(search(&conn, "msyh").unwrap().total, 1);
    }

    #[test]
    fn category_rename_does_not_affect_search() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_category(&conn, "cat-1", "餐饮", "expense");
        insert_txn(&conn, "tx-1", "acc-1", Some("cat-1"), None, "2026-02-01");
        rebuild_search_index(&conn).unwrap();
        // 分类名不在索引中：分类名/拼音均不可搜
        assert_eq!(search(&conn, "餐饮").unwrap().total, 0);
        assert_eq!(search(&conn, "cy").unwrap().total, 0);

        // 分类改名不触发重建：索引内容与结果均不变
        conn.execute(
            "UPDATE categories SET name='美食', updated_at='2026-02-02T00:00:00Z', version=version+1 WHERE id='cat-1'",
            [],
        )
        .unwrap();
        process_reindex_queue(&conn).unwrap();
        assert_eq!(search(&conn, "美食").unwrap().total, 0);
    }

    #[test]
    fn buy_transaction_indexed_with_account_name() {
        use crate::commands::transactions::insert_transaction;
        use crate::models::TransactionInput;
        let conn = setup();
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('acc-inv','美股账户','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES ('inst-1','AAPL','stock','苹果','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            [],
        )
        .unwrap();
        let input = TransactionInput {
            kind: "buy".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: "acc-inv".into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: Some("加仓".into()),
            date: "2026-01-10".into(),
            instrument_id: Some("inst-1".into()),
            quantity: Some(10.0),
            price_cents: Some(10000),
            fee_cents: Some(0),
            idempotency_key: None,
        };
        let id = insert_transaction(&conn, input).unwrap();
        // 写入路径不做同步索引（ADR-0004 决策 #14）：消费队列后立即可搜
        process_reindex_queue(&conn).unwrap();
        assert_eq!(search(&conn, "加仓").unwrap().total, 1);
        assert_eq!(
            search(&conn, "美股账户").unwrap().total,
            1,
            "投资交易按账户名可搜（全部交易类型覆盖）"
        );
        let _ = id;
    }
}
