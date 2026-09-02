//! IPC 命令壳 · 交易（Transaction，#403 域目录化 ADR-0056）：交易列表、单笔创建、批量创建、
//! 按 id 修改与删除五个命令。
//!
//! 只做参数解包、连接层事务边界与失效信号发射，不含业务语义；行为权威在
//! [`crate::transaction`]（核心交易域归位，#403 / ADR-0056）。注册路径与
//! 前端调用保持不变。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：写路径对备份域
//! 零感知，置脏/到期检查由写入口闭包在提交点单点执行。「是否发」失效信号的
//! 判定单点在 signals 映射（ADR-0044 / issue #331），壳层只归一化证据并转发。

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{
    CreateTransactionResult, TransactionInput, TransactionListFilter, TransactionListResult,
    UpdateTransactionInput,
};
use crate::signals::{WriteOp, emit_for};
use crate::transaction as transaction_domain;

#[tauri::command]
pub fn list_transactions(
    db: State<'_, DbState>,
    filter: Option<TransactionListFilter>,
) -> Result<TransactionListResult> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let filter = filter.unwrap_or_default();
    transaction_domain::list_transactions_internal(&conn, &filter)
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
    let write = db.write(|conn| transaction_domain::create(conn, input))?;
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
        db.write(|conn| transaction_domain::TransactionBatch::run(conn, inputs, false))?;
    emit_for(&app, WriteOp::BatchCreateTransactions, outcome.evidence);
    Ok(outcome.results)
}

/// 按 id 全字段替换一笔交易（issue #178 薄壳 IPC 命令；issue #331 起携带
/// 「即建商户」证据发射参考失效信号，ADR-0044）。
///
/// 行为权威是 [`transaction_domain::update`]（与 HTTP `PUT /api/v1/transactions/{id}`
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
    let evidence = db.write(|conn| transaction_domain::update(conn, &id, input.into()))?;
    // 仅即建商户发参考失效信号；仅命中复用（名字命中或直接带商户 id）零信号。
    emit_for(&app, WriteOp::UpdateTransaction, evidence);
    Ok(())
}

#[tauri::command]
pub fn delete_transaction(db: State<'_, DbState>, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：删除成功即置脏。
    // 删除编排入口（issue #229 / ADR-0033）：持仓清理与软删同事务，中途失败整体回滚。
    db.write(|conn| transaction_domain::delete(conn, &id))
}
