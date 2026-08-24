//! 预算（issue #91）：命令外壳 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `tests`：原内嵌测试外迁。
//!
//! 预算领域四个命令均为"命令外壳 + 内嵌 SQL"形态（主代码未超阈值，不拆核心逻辑），
//! 整体落于模块入口。对外暴露的命令经 `commands/mod.rs` 的 `pub use budget::*`
//! 重导出，注册路径与前端调用零改动。

#[cfg(test)]
mod tests;

use tauri::State;

use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Budget, BudgetInput, BudgetPeriod, BudgetProgress};

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

#[tauri::command]
pub fn budget_progress(db: State<'_, DbState>) -> Result<Vec<BudgetProgress>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT b.id,b.category_id,b.period,b.amount_cents,b.start_date,b.created_at,b.updated_at,b.version,b.device_id,b.is_deleted,c.name, \
         COALESCE((SELECT SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
                                    WHEN t.kind='refund' THEN -t.amount_native_cents \
                                    ELSE 0 END) \
                   FROM transactions t \
                   JOIN categories tc ON tc.id=t.category_id \
                   WHERE (tc.id=b.category_id OR tc.parent_id=b.category_id) \
                     AND t.is_deleted=0 \
                     AND substr(t.date,1,7)=substr(b.start_date,1,7)),0) \
         FROM budgets b LEFT JOIN categories c ON c.id=b.category_id \
         WHERE b.is_deleted=0 ORDER BY b.created_at",
        [],
    )
}
