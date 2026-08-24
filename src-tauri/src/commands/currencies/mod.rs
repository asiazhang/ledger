//! 币种（issue #92）：命令外壳 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `tests`：原内嵌测试外迁。
//!
//! 币种领域仅一个命令，主代码未超阈值，不拆核心逻辑，整体落于模块入口。
//! 对外暴露的命令与 `list_currencies_internal` 复用函数经 `commands/mod.rs` 的
//! `pub use currencies::*` 重导出，注册路径与前端/api_server 调用零改动。

#[cfg(test)]
mod tests;

use rusqlite::Connection;
use tauri::State;

use crate::db::DbState;
use crate::db::query::query_all;
use crate::error::Result;
use crate::models::Currency;

pub fn list_currencies_internal(conn: &Connection) -> Result<Vec<Currency>> {
    query_all(
        conn,
        "SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code",
        [],
    )
}

#[tauri::command]
pub fn list_currencies(db: State<'_, DbState>) -> Result<Vec<Currency>> {
    let conn = db
        .conn
        .lock()
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    list_currencies_internal(&conn)
}
