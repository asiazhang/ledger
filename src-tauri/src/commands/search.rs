//! IPC 命令壳 · 交易搜索（#403 域目录化 ADR-0056）：交易搜索命令。
//!
//! 只做参数解包与连接锁管理，不含业务语义；搜索查询权威在
//! [`crate::transaction::search`]（核心交易域归位，#403 / ADR-0056）。
//!
//! 命令 async 化（形状乙，spec #498 / #501）：DB 调用经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::transaction as transaction_domain;
use crate::transaction::{NotePinyinRepairReport, TransactionSearchResult};

/// IPC 命令：搜索交易（可选金额/日期筛选与关键字 AND 组合）。
/// 四个筛选参数与内部函数一一对应（issue #40），作为独立命令参数暴露，
/// 前端按 issue #41 契约以 camelCase 键名调用（Tauri 自动转 snake_case），
/// 故显式 allow `too_many_arguments`。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn search_transactions(
    db: State<'_, DbState>,
    query: String,
    page: Option<usize>,
    page_size: Option<usize>,
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Result<TransactionSearchResult> {
    let conn = db.conn.clone();
    run_db("search_transactions", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
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
    })
    .await
}

/// IPC 命令：备注拼音一键修复（issue #513）：显式回填全部积压并返回报告
/// （回填行数 / 是否收敛 / 失败原因）。领域权威在
/// [`crate::transaction::search`]（与搜索入口惰性回填同一实现，幂等）。
#[tauri::command]
pub async fn repair_note_pinyin(db: State<'_, DbState>) -> Result<NotePinyinRepairReport> {
    let conn = db.conn.clone();
    run_db("repair_note_pinyin", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        Ok(transaction_domain::repair_note_pinyin(&conn))
    })
    .await
}
