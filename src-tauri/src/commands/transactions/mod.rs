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
use crate::signals::{WriteOp, emit_for};

pub use read::*;

/// 行为层编排入口的 crate 外接缝（e2e/HTTP 与 IPC 同一实现，issue #228 / #229 / ADR-0033）：
/// `create` / `update` / `delete` 三入口——顺序契约、事务边界（嵌套感知）、守卫文案
/// 已全部内化在 [`behavior`]，调用方只传连接与输入、处理报错；创建/修改入口返回
/// 「是否即建商户」结果证据（ADR-0044 决策 4，issue #331），两壳据此经信号映射单点判定发射。
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

/// 创建单笔交易（issue #331 起携带「即建商户」证据发射参考失效信号，ADR-0044）。
#[tauri::command]
pub fn create_transaction(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: TransactionInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：锁连接 + 提交点置脏/到期检查单点。
    // 创建编排入口（issue #228 / ADR-0033）：行为层自持事务，中途失败整体回滚。
    let write = db.write(|conn| behavior::create(conn, input))?;
    // 信号在写事务提交成功后发射（映射单点判定，ADR-0044）：仅即建商户发参考失效信号，
    // 纯复用 / 不涉商户零信号。
    emit_for(&app, WriteOp::CreateTransaction, write.evidence);
    Ok(write.id)
}

/// 批量创建交易（issue #331 起按批聚合「任一行即建商户」发射参考失效信号）。
#[tauri::command]
pub fn create_transactions(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    inputs: Vec<TransactionInput>,
) -> Result<Vec<CreateTransactionResult>> {
    // 连接层统一写入口（ADR-0032，issue #245）：批次事务由 run 自持，提交点置脏/
    // 到期检查单点；整批回滚不置脏由写入口闭包失败语义保证。
    let outcome =
        db.write(|conn| crate::commands::batch::TransactionBatch::run(conn, inputs, false))?;
    emit_for(&app, WriteOp::BatchCreateTransactions, outcome.evidence);
    Ok(outcome.results)
}

/// 按 id 全字段替换一笔交易（issue #178 薄壳 IPC 命令；issue #331 起携带
/// 「即建商户」证据发射参考失效信号，ADR-0044）。
///
/// 行为权威是 [`behavior::update`]（与 HTTP `PUT /api/v1/transactions/{id}`
/// 同一入口，两条写路径行为不发散）；入参复用 [`UpdateTransactionInput`]
/// （不含幂等键，幂等键与内容哈希不可编辑）。不存在或已删除返回 `NotFound`，
/// 修改成功版本号递增。
#[tauri::command]
pub fn update_transaction(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: UpdateTransactionInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：锁连接 + 提交点置脏/到期检查单点。
    // 修改编排入口（issue #229 / ADR-0033）：行为层自持事务，中途失败整体回滚。
    let evidence = db.write(|conn| behavior::update(conn, &id, input.into()))?;
    // 仅即建商户发参考失效信号；仅命中复用（名字命中或直接带商户 id）零信号。
    emit_for(&app, WriteOp::UpdateTransaction, evidence);
    Ok(())
}

#[tauri::command]
pub fn delete_transaction(db: State<'_, DbState>, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：删除成功即置脏。
    // 删除编排入口（issue #229 / ADR-0033）：持仓清理与软删同事务，中途失败整体回滚。
    db.write(|conn| behavior::delete(conn, &id))
}
