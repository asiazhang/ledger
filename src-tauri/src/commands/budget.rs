//! IPC 命令壳 · 预算（Budget）。
//!
//! 只负责参数解包、连接层事务边界与命令注册；预算行为位于 [`crate::budget`]。

use chrono::Local;
use tauri::State;

use crate::budget as budget_domain;
use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{Budget, BudgetInput, BudgetProgress, BudgetUpdateInput};

#[tauri::command]
pub fn list_budgets(db: State<'_, DbState>) -> Result<Vec<Budget>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    budget_domain::list_budgets(&conn)
}

#[tauri::command]
pub fn create_budget(db: State<'_, DbState>, input: BudgetInput) -> Result<String> {
    db.write(|conn| budget_domain::crud::create_budget(conn, &input))
}

#[tauri::command]
pub fn update_budget(db: State<'_, DbState>, id: String, input: BudgetUpdateInput) -> Result<()> {
    db.write(|conn| budget_domain::crud::update_budget(conn, &id, input.amount_cents))
}

#[tauri::command]
pub fn delete_budget(db: State<'_, DbState>, id: String) -> Result<()> {
    db.write(|conn| budget_domain::crud::delete_budget(conn, &id))
}

#[tauri::command]
pub fn budget_progress(db: State<'_, DbState>) -> Result<Vec<BudgetProgress>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    budget_domain::budget_progress_rows(&conn, Local::now().date_naive())
}
