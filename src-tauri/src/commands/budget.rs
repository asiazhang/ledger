//! IPC 命令壳 · 预算（Budget）。
//!
//! 只负责参数解包与统一写入口一行调用；预算行为位于 [`crate::budget`]。
//!
//! 全部命令 async 化（形状乙，spec #498 / #502）；写命令经壳层统一写入口
//! [`crate::write_entry::write_entry`]（ADR-0073）：仪式（锁、事务、置脏、信号）
//! 内化单点。预算写入刻意零信号（映射单点显式登记），身份仍随入口流动——
//! 未来补信号时天然生效。
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
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

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
pub async fn create_budget(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: BudgetInput,
) -> Result<String> {
    let conn = db.conn.clone();
    write_entry(
        "create_budget",
        conn,
        Some(&app),
        WriteOp::CreateBudget,
        move |conn| budget_domain::crud::create_budget(conn, &input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_budget(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: BudgetUpdateInput,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "update_budget",
        conn,
        Some(&app),
        WriteOp::UpdateBudget,
        move |conn| {
            budget_domain::crud::update_budget(conn, &id, input.amount_cents).map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn delete_budget(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "delete_budget",
        conn,
        Some(&app),
        WriteOp::DeleteBudget,
        move |conn| budget_domain::crud::delete_budget(conn, &id).map(Outcome::Silent),
    )
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
