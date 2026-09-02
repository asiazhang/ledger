//! 交易搜索（issue #196，修订 ADR-0004）：Rust 全量扫描 + 统一模糊搜索语义。
//!
//! - 匹配语义（统一模糊搜索规格，issue #195 / ADR-0027）：输入按空白切词，
//!   词条之间 AND；每词条对可搜索字段（备注、转出账户名）判定——
//!   命中 = 原文连续子串（大小写不敏感）∨ 该字段拼音首字母串的子序列（大小写不敏感）。
//! - 实现：SQL 取候选（非删除交易的备注 + 转出账户名，金额/日期过滤仍在 SQL）→
//!   Rust 逐条按语义契约过滤 → 交易日期降序分页返回。无任何索引，写入立即可搜。
//! - 结果不附 `stale` 标志：搜索为同步全量匹配，无任何索引滞后可言。
//!
//! 目录组织：
//! - `query`：查询执行——SQL 候选 + Rust 过滤 + 内存分页；统一模糊搜索语义的
//!   纯文本逻辑已随 #401 域目录化归入核心交易域（`transaction::search_text`，
//!   唯一定义点见核心域 TransactionSearch 词条，ADR-0027），本壳按域入口消费。

mod query;
#[cfg(test)]
mod tests;

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::TransactionSearchResult;

pub use query::search_transactions_internal;

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
