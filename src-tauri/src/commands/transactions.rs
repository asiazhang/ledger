//! IPC 命令壳 · 交易（Transaction，#403 域目录化 ADR-0056）：交易列表、单笔创建、批量创建、
//! 按 id 修改与删除五个命令。
//!
//! 只做参数解包与统一写入口/读 helper 一行调用，不含业务语义；行为权威在
//! [`crate::transaction`]（核心交易域归位，#403 / ADR-0056）。注册路径与
//! 前端调用保持不变。
//!
//! 写命令经壳层统一写入口 [`crate::write_entry::write_entry`]（ADR-0073）：
//! 仪式（锁、事务、置脏、信号）内化单点，「即建商户」证据随闭包返回必达，
//! 判定单点在 signals 映射（ADR-0044 / issue #331）；读命令经 `run_db`
//!（形状乙，spec #498 / #502）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::signals::WriteOp;
use crate::transaction as transaction_domain;
use crate::transaction::{
    CreateTransactionResult, TransactionInput, TransactionListFilter, TransactionListResult,
    UpdateTransactionInput,
};
use crate::write_entry::{Outcome, write_entry};

#[tauri::command]
pub async fn list_transactions(
    db: State<'_, DbState>,
    filter: Option<TransactionListFilter>,
) -> Result<TransactionListResult> {
    let conn = db.conn.clone();
    run_db("list_transactions", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let filter = filter.unwrap_or_default();
        transaction_domain::list_transactions_internal(&conn, &filter)
    })
    .await
}

/// 创建单笔交易（issue #331 起携带「即建商户」证据发射参考失效信号，ADR-0044）。
#[tauri::command]
pub async fn create_transaction(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: TransactionInput,
) -> Result<String> {
    // 创建编排入口（issue #228 / ADR-0033）：行为层自持事务，中途失败整体回滚。
    let conn = db.conn.clone();
    write_entry(
        "create_transaction",
        conn,
        Some(&app),
        WriteOp::CreateTransaction,
        move |conn| {
            transaction_domain::create(conn, input)
                .map(|write| Outcome::Evidenced(write.id, write.evidence))
        },
    )
    .await
}

/// 批量创建交易（issue #331 起按批聚合「任一行即建商户」发射参考失效信号）。
#[tauri::command]
pub async fn create_transactions(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    inputs: Vec<TransactionInput>,
) -> Result<Vec<CreateTransactionResult>> {
    // 批次事务由 run 自持（issue #245），提交点置脏/到期检查单点；整批回滚不置脏
    // 由写入口闭包失败语义保证。
    let conn = db.conn.clone();
    write_entry(
        "create_transactions",
        conn,
        Some(&app),
        WriteOp::BatchCreateTransactions,
        move |conn| {
            transaction_domain::TransactionBatch::run(conn, inputs, false)
                .map(|outcome| Outcome::Evidenced(outcome.results, outcome.evidence))
        },
    )
    .await
}

/// 按 id 全字段替换一笔交易（issue #178 薄壳 IPC 命令；issue #331 起携带
/// 「即建商户」证据发射参考失效信号，ADR-0044）。
///
/// 行为权威是 [`transaction_domain::update`]（与 HTTP `PUT /api/v1/transactions/{id}`
/// 同一入口，两条写路径行为不发散）；入参复用 [`UpdateTransactionInput`]
/// （不含幂等键，幂等键与内容哈希不可编辑）。不存在或已删除返回 `NotFound`，
/// 修改成功版本号递增。
#[tauri::command]
pub async fn update_transaction(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: UpdateTransactionInput,
) -> Result<()> {
    // 修改编排入口（issue #229 / ADR-0033）：行为层自持事务，中途失败整体回滚。
    let conn = db.conn.clone();
    write_entry(
        "update_transaction",
        conn,
        Some(&app),
        WriteOp::UpdateTransaction,
        move |conn| {
            transaction_domain::update(conn, &id, input.into())
                .map(|evidence| Outcome::Evidenced((), evidence))
        },
    )
    .await
}

#[tauri::command]
pub async fn delete_transaction(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    // 删除编排入口（issue #229 / ADR-0033）：持仓清理与软删同事务，中途失败整体回滚。
    let conn = db.conn.clone();
    write_entry(
        "delete_transaction",
        conn,
        Some(&app),
        WriteOp::DeleteTransaction,
        move |conn| transaction_domain::delete(conn, &id).map(Outcome::Silent),
    )
    .await
}
