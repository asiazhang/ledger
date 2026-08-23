//! 交易搜索（ADR-0004）：FTS5 索引维护与查询。
//!
//! - 可搜索内容：备注 + 账户名 + 分类名 + 三者拼音首字母（仅首字母缩写、小写）。
//! - 匹配语义：整词匹配 + 拼音首字母匹配 + 前缀通配；词条间 AND、词条内原词/前缀 OR。
//! - 索引维护：交易创建/删除后应用层即时重建；账户/分类改名由触发器入队
//!   `search_reindex_queue`，消费后批量重建；启动时按文档数对账兜底全量重建。

use rusqlite::Connection;
use rusqlite::OptionalExtension;
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

/// 拼接可搜索内容：`备注 转出账户名 转入账户名 分类名 备注拼音 转出账户拼音 转入账户拼音 分类拼音`。
/// 转账同时携带转出/转入账户名；空字段跳过；所有字段为空时返回空串（仍保留文档行）。
pub fn build_search_content(
    note: Option<&str>,
    account_name: &str,
    to_account_name: &str,
    category_name: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(8);
    let text_parts = [
        note,
        Some(account_name),
        Some(to_account_name),
        category_name,
    ];
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

/// 服务端分页搜索交易。整词/前缀匹配（FTS5 MATCH），JOIN 回主表过滤软删除，
/// 排序按相关度 rank 优先、日期倒序次之、id 兜底（与交易列表先例一致，防同秒
/// 批量写入翻页漂移）；返回当前页与命中总数。
pub fn search_transactions_internal(
    conn: &Connection,
    query: &str,
    page: usize,
    page_size: usize,
) -> Result<TransactionSearchResult> {
    let match_expr = build_match_query(query);
    if match_expr.is_empty() {
        return Ok(TransactionSearchResult {
            items: Vec::new(),
            total: 0,
        });
    }
    let page = page.max(1);
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
    // offset 用 saturating 运算 + try_from 钳制，防极端输入（usize::MAX）产生
    // debug 构建 panic 或 SQLite datatype mismatch（与 list_transactions 先例一致）
    let offset =
        i64::try_from(page.saturating_sub(1).saturating_mul(page_size)).unwrap_or(i64::MAX);

    let join = "FROM search_transactions s \
                JOIN transactions t ON s.transaction_id = t.id \
                JOIN accounts a ON t.account_id = a.id \
                LEFT JOIN categories c ON t.category_id = c.id \
                WHERE search_transactions MATCH ?1 \
                AND t.is_deleted = 0 \
                AND a.is_deleted = 0 \
                AND (c.is_deleted = 0 OR c.id IS NULL)";

    let total: i64 = conn.query_row(&format!("SELECT COUNT(*) {join}"), [&match_expr], |r| {
        r.get(0)
    })?;

    let items = query_all(
        conn,
        &format!(
            "SELECT t.id,t.kind,t.amount_cents,t.currency_code,t.amount_native_cents,t.account_id,\
             t.to_account_id,t.category_id,t.refund_of_transaction_id,t.note,t.date,t.created_at,\
             t.updated_at,t.version,t.device_id,t.is_deleted \
             {join} \
             ORDER BY rank DESC, t.date DESC, t.created_at DESC, t.id DESC \
             LIMIT ?2 OFFSET ?3"
        ),
        rusqlite::params![match_expr, page_size as i64, offset],
    )?;

    Ok(TransactionSearchResult { items, total })
}

#[tauri::command]
pub fn search_transactions(
    db: State<'_, DbState>,
    query: String,
    page: Option<usize>,
    page_size: Option<usize>,
) -> Result<TransactionSearchResult> {
    let conn = db
        .conn
        .lock()
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    search_transactions_internal(&conn, &query, page.unwrap_or(1), page_size.unwrap_or(20))
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

/// 为**新建**交易直接插入 FTS 文档（免查重：不扫描表判断是否已存在，O(1)）。
/// 仅用于刚插入、确定没有文档的交易（如 `insert_transaction` 钩子）；
/// 批量导入下避免每次插入都全表扫描（UNINDEXED 列无索引可走）。
pub fn insert_index_document(conn: &Connection, transaction_id: &str) -> Result<()> {
    let Some(payload) = read_index_payload(conn, transaction_id)? else {
        return Ok(());
    };
    if payload.is_deleted {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO search_transactions(content, transaction_id) VALUES(?1, ?2)",
        rusqlite::params![payload.content, transaction_id],
    )?;
    Ok(())
}

/// 索引载荷：可搜索内容与软删除标志。
struct IndexPayload {
    content: String,
    is_deleted: bool,
}

/// 读取交易的索引载荷。交易不存在返回 None。
/// 内容 = 备注 + 转出账户名 + 转入账户名 + 分类名 + 四者拼音首字母。
fn read_index_payload(conn: &Connection, transaction_id: &str) -> Result<Option<IndexPayload>> {
    let row = conn
        .query_row(
            "SELECT t.note, COALESCE(a.name,''), COALESCE(a2.name,''), COALESCE(c.name,''), \
             t.is_deleted \
             FROM transactions t \
             LEFT JOIN accounts a ON t.account_id = a.id \
             LEFT JOIN accounts a2 ON t.to_account_id = a2.id \
             LEFT JOIN categories c ON t.category_id = c.id \
             WHERE t.id = ?1",
            [transaction_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?;
    Ok(row.map(
        |(note, account_name, to_account_name, category_name, is_deleted)| IndexPayload {
            content: build_search_content(
                note.as_deref(),
                &account_name,
                &to_account_name,
                category_name.as_deref(),
            ),
            is_deleted: is_deleted != 0,
        },
    ))
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
    fn build_content_joins_note_account_category_and_initials() {
        let content = build_search_content(Some("吃饭"), "招商银行", "", Some("餐饮"));
        assert_eq!(content, "吃饭 招商银行 餐饮 cf zsyh cy");
    }

    #[test]
    fn build_content_includes_transfer_to_account() {
        let content = build_search_content(Some("转账"), "现金", "银行", None);
        assert_eq!(content, "转账 现金 银行 zz xj yh");
    }

    #[test]
    fn build_content_skips_empty_fields() {
        assert_eq!(build_search_content(None, "现金", "", None), "现金 xj");
        assert_eq!(
            build_search_content(Some("   "), "现金", "", None),
            "现金 xj"
        );
        assert_eq!(build_search_content(None, "", "", None), "");
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
    fn search_matches_note_account_category_and_pinyin() {
        let conn = setup();
        insert_account(&conn, "acc-1", "招商银行", "bank", "CNY");
        insert_account(&conn, "acc-2", "现金", "cash", "CNY");
        insert_category(&conn, "cat-1", "餐饮", "expense");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("吃饭"), "2026-02-01");
        insert_txn(&conn, "tx-2", "acc-2", Some("cat-1"), None, "2026-02-02");
        rebuild_search_index(&conn).unwrap();

        // 备注整词
        let r = search_transactions_internal(&conn, "吃饭", 1, 20).unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.items[0].id, "tx-1");
        // 拼音首字母 cf
        assert_eq!(
            search_transactions_internal(&conn, "cf", 1, 20)
                .unwrap()
                .total,
            1
        );
        // 账户名整词
        assert_eq!(
            search_transactions_internal(&conn, "招商", 1, 20)
                .unwrap()
                .total,
            1
        );
        // 账户名拼音 zsyh
        assert_eq!(
            search_transactions_internal(&conn, "zsyh", 1, 20)
                .unwrap()
                .total,
            1
        );
        // 分类名整词
        assert_eq!(
            search_transactions_internal(&conn, "餐饮", 1, 20)
                .unwrap()
                .total,
            1
        );
        // 前缀通配：吃 → 吃饭
        assert_eq!(
            search_transactions_internal(&conn, "吃", 1, 20)
                .unwrap()
                .total,
            1
        );
        // 整词不命中子串
        assert_eq!(
            search_transactions_internal(&conn, "商银", 1, 20)
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn search_multi_keyword_and_combination() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        insert_txn(&conn, "tx-2", "acc-1", None, Some("晚餐"), "2026-02-02");
        rebuild_search_index(&conn).unwrap();

        // 两个词条同时命中才返回（AND 语义）
        let r = search_transactions_internal(&conn, "午餐 现金", 1, 20).unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.items[0].id, "tx-1");
        let r = search_transactions_internal(&conn, "午餐 晚餐", 1, 20).unwrap();
        assert_eq!(r.total, 0);
    }

    #[test]
    fn search_excludes_soft_deleted() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        rebuild_search_index(&conn).unwrap();
        assert_eq!(
            search_transactions_internal(&conn, "午餐", 1, 20)
                .unwrap()
                .total,
            1
        );

        // 软删除后索引文档被移除，搜索结果消失
        conn.execute(
            "UPDATE transactions SET is_deleted=1, updated_at='2026-02-03T00:00:00Z', version=version+1 WHERE id='tx-1'",
            [],
        )
        .unwrap();
        process_reindex_queue(&conn).unwrap();
        assert_eq!(
            search_transactions_internal(&conn, "午餐", 1, 20)
                .unwrap()
                .total,
            0
        );
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

        let r = search_transactions_internal(&conn, "午餐", 1, 20).unwrap();
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

        let r = search_transactions_internal(&conn, "午餐", 1, 2).unwrap();
        assert_eq!(r.total, 5);
        assert_eq!(r.items.len(), 2);
        assert_eq!(r.items[0].id, "tx-5", "日期倒序第一页首条应为最新");

        let r = search_transactions_internal(&conn, "午餐", 3, 2).unwrap();
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].id, "tx-1");
    }

    #[test]
    fn search_transfer_by_to_account_name() {
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
        };
        let id = insert_transaction(&conn, input).unwrap();
        // 转出账户名、转入账户名（含拼音首字母）均可搜
        assert_eq!(
            search_transactions_internal(&conn, "现金", 1, 20)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            search_transactions_internal(&conn, "招商", 1, 20)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            search_transactions_internal(&conn, "zsyh", 1, 20)
                .unwrap()
                .total,
            1
        );
        let _ = id;
    }

    #[test]
    fn search_extreme_page_inputs_do_not_panic() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        rebuild_search_index(&conn).unwrap();

        // 极端输入：usize::MAX 页/页大小不 panic、不破坏 total；page=0 钳制为 1
        let r = search_transactions_internal(&conn, "午餐", usize::MAX, usize::MAX).unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.items.len(), 0);
        let r = search_transactions_internal(&conn, "午餐", 0, 0).unwrap();
        assert_eq!(r.total, 1);
        assert_eq!(r.items.len(), 1);
    }

    #[test]
    fn search_empty_query_and_special_chars() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        rebuild_search_index(&conn).unwrap();

        assert_eq!(
            search_transactions_internal(&conn, "", 1, 20)
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            search_transactions_internal(&conn, "   ", 1, 20)
                .unwrap()
                .total,
            0
        );
        // 特殊字符不报错、不误命中
        let r = search_transactions_internal(&conn, "午餐 AND 现金 OR (NOT)", 1, 20).unwrap();
        assert_eq!(r.total, 0);
        let r = search_transactions_internal(&conn, "午餐\"", 1, 20).unwrap();
        assert_eq!(r.total, 1, "剥离引号后仍命中");
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
        let r = search_transactions_internal(&conn, "退款", 1, 20).unwrap();
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
        assert_eq!(
            search_transactions_internal(&conn, "午餐", 1, 20)
                .unwrap()
                .total,
            1
        );
    }

    #[test]
    fn reconcile_rebuilds_when_counts_mismatch() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, Some("午餐"), "2026-02-01");
        // FTS 为空（存量），counts 不匹配 → 全量重建
        reconcile_search_index(&conn).unwrap();
        assert_eq!(
            search_transactions_internal(&conn, "午餐", 1, 20)
                .unwrap()
                .total,
            1
        );
        // 再次对账：一致 → 走队列消费，结果不变
        reconcile_search_index(&conn).unwrap();
        assert_eq!(
            search_transactions_internal(&conn, "午餐", 1, 20)
                .unwrap()
                .total,
            1
        );
    }

    #[test]
    fn account_rename_updates_searchable_content() {
        let conn = setup();
        insert_account(&conn, "acc-1", "招商银行", "bank", "CNY");
        insert_txn(&conn, "tx-1", "acc-1", None, None, "2026-02-01");
        rebuild_search_index(&conn).unwrap();
        assert_eq!(
            search_transactions_internal(&conn, "招商", 1, 20)
                .unwrap()
                .total,
            1
        );

        // 账户改名：触发器入队，消费后新名称生效
        conn.execute(
            "UPDATE accounts SET name='民生银行', updated_at='2026-02-02T00:00:00Z', version=version+1 WHERE id='acc-1'",
            [],
        )
        .unwrap();
        process_reindex_queue(&conn).unwrap();
        assert_eq!(
            search_transactions_internal(&conn, "招商", 1, 20)
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            search_transactions_internal(&conn, "民生", 1, 20)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            search_transactions_internal(&conn, "msyh", 1, 20)
                .unwrap()
                .total,
            1
        );
    }

    #[test]
    fn category_rename_updates_searchable_content() {
        let conn = setup();
        insert_account(&conn, "acc-1", "现金", "cash", "CNY");
        insert_category(&conn, "cat-1", "餐饮", "expense");
        insert_txn(&conn, "tx-1", "acc-1", Some("cat-1"), None, "2026-02-01");
        rebuild_search_index(&conn).unwrap();
        assert_eq!(
            search_transactions_internal(&conn, "餐饮", 1, 20)
                .unwrap()
                .total,
            1
        );

        conn.execute(
            "UPDATE categories SET name='美食', updated_at='2026-02-02T00:00:00Z', version=version+1 WHERE id='cat-1'",
            [],
        )
        .unwrap();
        process_reindex_queue(&conn).unwrap();
        assert_eq!(
            search_transactions_internal(&conn, "餐饮", 1, 20)
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            search_transactions_internal(&conn, "美食", 1, 20)
                .unwrap()
                .total,
            1
        );
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
        };
        let id = insert_transaction(&conn, input).unwrap();
        // insert_transaction 已即时重建索引
        assert_eq!(
            search_transactions_internal(&conn, "加仓", 1, 20)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            search_transactions_internal(&conn, "美股账户", 1, 20)
                .unwrap()
                .total,
            1,
            "投资交易按账户名可搜（全部交易类型覆盖）"
        );
        let _ = id;
    }
}
