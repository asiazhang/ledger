//! IPC 命令壳 · 币种（Currency）。
//!
//! 只负责参数解包与命令注册；币种为种子权威参考数据，无写命令、无失效信号，
//! 清单查询实现位于 [`crate::currencies`]。注册路径与前端调用保持不变。

use tauri::State;

use crate::currencies as currency_domain;
use crate::currencies::Currency;
use crate::db::DbState;
use crate::error::{AppError, Result};

/// 币种清单：全部种子币种按 `code` 排序。
#[tauri::command]
pub fn list_currencies(db: State<'_, DbState>) -> Result<Vec<Currency>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    currency_domain::list_currencies(&conn)
}
