//! 报表（issue #92）：命令外壳 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `tests`：原内嵌测试外迁。
//!
//! 报表领域两个命令均为"命令外壳 + 内嵌 SQL"形态（主代码未超阈值，不拆核心逻辑），
//! 整体落于模块入口。对外暴露的命令经 `commands/mod.rs` 的 `pub use reports::*`
//! 重导出，注册路径与前端调用零改动。

#[cfg(test)]
mod tests;

use tauri::State;

use crate::db::DbState;
use crate::db::query::query_all;
use crate::error::{AppError, Result};
use crate::models::{CategoryShare, MonthlySummary};

#[tauri::command]
pub fn monthly_summary(db: State<'_, DbState>, year: i64) -> Result<Vec<MonthlySummary>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT substr(date,1,7) AS month, \
         SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END) AS income, \
         SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END) AS expense, \
         SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) AS refund \
         FROM transactions WHERE substr(date,1,4)=?1 AND is_deleted=0 \
         GROUP BY month ORDER BY month",
        rusqlite::params![format!("{year}")],
    )
}

#[tauri::command]
pub fn category_shares(
    db: State<'_, DbState>,
    kind: String,
    month: Option<String>,
) -> Result<Vec<CategoryShare>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let (kinds, sign_expr) = if kind == "expense" {
        (
            "'expense','refund'",
            "CASE WHEN t.kind='expense' THEN t.amount_native_cents \
              WHEN t.kind='refund' THEN -t.amount_native_cents ELSE 0 END",
        )
    } else {
        ("'income'", "t.amount_native_cents")
    };
    let mut sql = format!(
        "SELECT t.category_id, COALESCE(c.name,'未分类'), SUM({sign_expr}) AS net \
         FROM transactions t LEFT JOIN categories c ON c.id=t.category_id \
         WHERE t.kind IN ({kinds}) AND t.is_deleted=0"
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(m) = month {
        sql.push_str(" AND substr(t.date,1,7)=?1");
        params_vec.push(Box::new(m));
    }
    sql.push_str(" GROUP BY t.category_id ORDER BY net DESC");
    let params_ref: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    query_all(&conn, &sql, params_ref.as_slice())
}
