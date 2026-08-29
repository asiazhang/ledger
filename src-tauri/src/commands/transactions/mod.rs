mod behavior;
mod read;
#[cfg(test)]
mod tests;

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{
    CreateTransactionResult, TransactionInput, TransactionListFilter, TransactionListResult,
    UpdateTransactionInput,
};

pub use read::*;

/// 行为层编排入口的 crate 外接缝（e2e/HTTP 与 IPC 同一实现，issue #228 / #229 / ADR-0030）：
/// `create` / `update` / `delete` 三入口——顺序契约、事务边界（嵌套感知）、守卫文案
/// 已全部内化在 [`behavior`]，调用方只传连接与输入、处理报错。
pub use behavior::{
    create as create_transaction_internal, delete as delete_transaction_internal,
    update as update_transaction_internal,
};

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
    // 连接层统一写入口（ADR-0032）：锁连接 + 提交点置脏/到期检查单点。
    // 创建编排入口（issue #228 / ADR-0030）：行为层自持事务，中途失败整体回滚。
    db.write(|conn| behavior::create(conn, input))
}

#[tauri::command]
pub fn create_transactions(
    db: State<'_, DbState>,
    inputs: Vec<TransactionInput>,
) -> Result<Vec<CreateTransactionResult>> {
    // 批量编排（含提交点置脏）留在 TransactionBatch::run，随 #245 迁入写入口。
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::commands::batch::TransactionBatch::run(&conn, inputs, false)
}

/// 按 id 全字段替换一笔交易（issue #178 薄壳 IPC 命令）。
///
/// 行为权威是 [`behavior::update`]（与 HTTP `PUT /api/v1/transactions/{id}`
/// 同一入口，两条写路径行为不发散）；入参复用 [`UpdateTransactionInput`]
/// （不含幂等键，幂等键与内容哈希不可编辑）。不存在或已删除返回 `NotFound`，
/// 修改成功版本号递增。
#[tauri::command]
pub fn update_transaction(
    db: State<'_, DbState>,
    id: String,
    input: UpdateTransactionInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：锁连接 + 提交点置脏/到期检查单点。
    // 修改编排入口（issue #229 / ADR-0030）：行为层自持事务，中途失败整体回滚。
    db.write(|conn| behavior::update(conn, &id, input.into()))
}

#[tauri::command]
pub fn delete_transaction(db: State<'_, DbState>, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：删除成功即置脏。
    // 删除编排入口（issue #229 / ADR-0030）：持仓清理与软删同事务，中途失败整体回滚。
    db.write(|conn| behavior::delete(conn, &id))
}
