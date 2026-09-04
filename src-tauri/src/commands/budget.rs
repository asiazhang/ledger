//! IPC 命令壳 · 预算（Budget）。
//!
//! 只负责参数解包、连接层事务边界与命令注册；预算行为位于 [`crate::budget`]。
//!
//! 全部命令 async 化（形状乙，spec #498 / #502）：DB 调用经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程；
//! 写路径仍在连接层统一写入口内置脏（ADR-0032 语义零改动），对用户外部行为不变。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use chrono::Local;
use tauri::State;

use crate::budget as budget_domain;
use crate::budget::{Budget, BudgetInput, BudgetProgress, BudgetUpdateInput};
use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};

#[tauri::command]
pub async fn list_budgets(db: State<'_, DbState>) -> Result<Vec<Budget>> {
    let conn = db.conn.clone();
    run_db("list_budgets", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        budget_domain::list_budgets(&conn)
    })
    .await
}

#[tauri::command]
pub async fn create_budget(db: State<'_, DbState>, input: BudgetInput) -> Result<String> {
    let conn = db.conn.clone();
    run_db("create_budget", move || {
        crate::db::write(&conn, |conn| {
            budget_domain::crud::create_budget(conn, &input)
        })
    })
    .await
}

#[tauri::command]
pub async fn update_budget(
    db: State<'_, DbState>,
    id: String,
    input: BudgetUpdateInput,
) -> Result<()> {
    let conn = db.conn.clone();
    run_db("update_budget", move || {
        crate::db::write(&conn, |conn| {
            budget_domain::crud::update_budget(conn, &id, input.amount_cents)
        })
    })
    .await
}

#[tauri::command]
pub async fn delete_budget(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.clone();
    run_db("delete_budget", move || {
        crate::db::write(&conn, |conn| budget_domain::crud::delete_budget(conn, &id))
    })
    .await
}

#[tauri::command]
pub async fn budget_progress(db: State<'_, DbState>) -> Result<Vec<BudgetProgress>> {
    let conn = db.conn.clone();
    run_db("budget_progress", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        budget_domain::budget_progress_rows(&conn, Local::now().date_naive())
    })
    .await
}
