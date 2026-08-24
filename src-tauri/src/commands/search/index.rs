//! FTS 索引维护：重建队列消费、全量重建、启动对账、后台刷新线程。

use rusqlite::Connection;
use rusqlite::OptionalExtension;
use std::sync::{Arc, Mutex};

use crate::error::Result;

use super::text::build_search_content;

/// 后台刷新周期：固定间隔轮询搜索重建队列（秒）。
/// 时效性要求低（用户可接受分钟级滞后），周期内写入不立即可搜。
const REFRESH_INTERVAL_SECS: u64 = 60;

/// 搜索重建队列是否非空（存在尚未消费的写入）。
pub(super) fn reindex_queue_pending(conn: &Connection) -> Result<bool> {
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
