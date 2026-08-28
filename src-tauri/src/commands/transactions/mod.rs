mod behavior;
mod read;
#[cfg(test)]
mod tests;
mod write;

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{
    CreateTransactionResult, TransactionInput, TransactionListFilter, TransactionListResult,
    UpdateTransactionInput,
};

pub use read::*;
pub use write::*;

#[tauri::command]
pub fn list_transactions(
    db: State<'_, DbState>,
    filter: Option<TransactionListFilter>,
) -> Result<TransactionListResult> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let filter = filter.unwrap_or_default();
    list_transactions_internal(&conn, &filter)
}

#[tauri::command]
pub fn create_transaction(db: State<'_, DbState>, input: TransactionInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    insert_transaction(&conn, input)
}

#[tauri::command]
pub fn create_transactions(
    db: State<'_, DbState>,
    inputs: Vec<TransactionInput>,
) -> Result<Vec<CreateTransactionResult>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::commands::batch::TransactionBatch::run(&conn, inputs, false)
}

/// 按 id 全字段替换一笔交易（issue #178 薄壳 IPC 命令）。
///
/// 行为权威是 [`update_transaction_internal`]（与 HTTP `PUT /api/v1/transactions/{id}`
/// 同一入口，两条写路径行为不发散）；入参复用 [`UpdateTransactionInput`]
/// （不含幂等键，幂等键与内容哈希不可编辑）。不存在或已删除返回 `NotFound`，
/// 修改成功版本号递增。
#[tauri::command]
pub fn update_transaction(
    db: State<'_, DbState>,
    id: String,
    input: UpdateTransactionInput,
) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    update_transaction_internal(&conn, &id, input.into())
}

#[tauri::command]
pub fn delete_transaction(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    delete_transaction_internal(&conn, &id)
}
