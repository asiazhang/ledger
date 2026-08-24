//! 交易搜索（ADR-0004）：FTS5 索引维护与查询。
//!
//! - 可搜索内容：备注 + 账户名 + 二者拼音首字母（仅首字母缩写、小写）。
//! - 匹配语义：整词匹配 + 拼音首字母匹配 + 前缀通配；词条间 AND、词条内原词/前缀 OR。
//! - 索引维护（ADR-0004 决策 #14 刷新策略）：**后台定时刷新**——交易写入路径不做任何
//!   同步索引工作（界面操作零索引开销），由触发器纯 SQL 入队 `search_reindex_queue`，
//!   后台线程固定周期（默认 60s）消费队列批量重建；批量导入完成后在命令内立即消费一次；
//!   启动时按文档数对账兜底全量重建。
//! - 搜索结果附 `stale` 标志：队列非空（存在未消费写入）时 true，供前端提示索引可能滞后。
//!
//! 目录组织（issue #88）：
//! - `text`：纯文本逻辑——拼音首字母/可搜索内容组装/FTS 查询构建；
//! - `query`：查询执行——服务端分页搜索内部实现；
//! - `index`：索引维护——队列消费/全量重建/启动对账/后台刷新线程；
//! - `tests`：原内嵌测试外迁。
//!
//! 依赖方经 `pub use` 重导出保持 `commands::search::*` 路径稳定（启动对账、
//! 导入即时消费、后台刷新线程与 BDD step 均零改动）。

mod index;
mod query;
#[cfg(test)]
mod tests;
mod text;

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::TransactionSearchResult;

pub use index::{
    delete_index_document, process_reindex_queue, rebuild_search_index, reconcile_search_index,
    reindex_transaction, start_search_refresh_thread,
};
pub use query::search_transactions_internal;
pub use text::{build_match_query, build_search_content, pinyin_initials};

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
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
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
