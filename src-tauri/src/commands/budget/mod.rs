//! 预算（issue #91 创建，issue #58 迁移 Amount 口径）：命令外壳 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `tests`：针对模块接口的测试（期望值由度量矩阵逐行求和得出，不复制生产 SQL）。
//!
//! 金额口径由 `transaction::amount` 的 kind→度量矩阵单一真源驱动：
//! 预算 spent = `expense_net`（毛支出 − 退款），与报表分类净值口径一致。
//!
//! 命令层为薄壳（锁 DbState 后调核心函数），核心函数吃 `&Connection` 可直接单测。
//! 对外暴露的命令经 `commands/mod.rs` 的 `pub use budget::*` 重导出，
//! 注册路径与前端调用零改动。

#[cfg(test)]
mod tests;

use rusqlite::Connection;
use tauri::State;

use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Budget, BudgetInput, BudgetPeriod, BudgetProgress};
use crate::transaction::amount::{Measure, contributing_kinds_sql, expense_net_expr};

#[tauri::command]
pub fn list_budgets(db: State<'_, DbState>) -> Result<Vec<Budget>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted \
         FROM budgets WHERE is_deleted=0 ORDER BY created_at",
        [],
    )
}

#[tauri::command]
pub fn create_budget(db: State<'_, DbState>, input: BudgetInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    let period = input.period.unwrap_or(BudgetPeriod::Monthly).to_string();
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
        rusqlite::params![
            id,
            input.category_id,
            period,
            input.amount_cents,
            input.start_date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn delete_budget(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "UPDATE budgets SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

/// 预算进度核心：spent = `expense_net`（毛支出 − 退款，退款冲减支出），
/// 与报表分类净值同口径；参与 kind 由矩阵导出（不含 buy/sell 等投资类）。
pub fn budget_progress_rows(conn: &Connection) -> Result<Vec<BudgetProgress>> {
    let kinds = contributing_kinds_sql(Measure::ExpenseNet);
    let sql = format!(
        "SELECT b.id,b.category_id,b.period,b.amount_cents,b.start_date,b.created_at,b.updated_at,b.version,b.device_id,b.is_deleted,c.name, \
         COALESCE((SELECT SUM({expense_net}) \
                   FROM transactions t \
                   JOIN categories tc ON tc.id=t.category_id \
                   WHERE (tc.id=b.category_id OR tc.parent_id=b.category_id) \
                     AND t.kind IN ({kinds}) \
                     AND t.is_deleted=0 \
                     AND substr(t.date,1,7)=substr(b.start_date,1,7)),0) \
         FROM budgets b LEFT JOIN categories c ON c.id=b.category_id \
         WHERE b.is_deleted=0 ORDER BY b.created_at",
        expense_net = expense_net_expr("t"),
    );
    query_all(conn, &sql, [])
}

#[tauri::command]
pub fn budget_progress(db: State<'_, DbState>) -> Result<Vec<BudgetProgress>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    budget_progress_rows(&conn)
}
