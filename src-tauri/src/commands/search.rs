//! IPC 命令壳 · 交易搜索（#403 域目录化 ADR-0056）：交易搜索命令。
//!
//! 只做参数解包与连接锁管理，不含业务语义；搜索查询权威在
//! [`crate::transaction::search`]（核心交易域归位，#403 / ADR-0056）。

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::TransactionSearchResult;
use crate::transaction as transaction_domain;

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
    transaction_domain::search_transactions_internal(
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
