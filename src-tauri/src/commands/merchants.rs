//! IPC 命令壳 · 商户（Merchant）。
//!
//! 只负责参数解包、事务边界与失效信号发射；商户字典行为位于 [`crate::merchants`]。
//! 注册路径与前端调用保持不变。
//!
//! 全部命令 async 化（形状乙，spec #498 / #501）：DB 调用经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程；
//! 写路径仍在连接层统一写入口内置脏（ADR-0032 语义零改动）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::merchants as merchant_domain;
use crate::merchants::{Merchant, MerchantInput, MerchantTransactionCount, MerchantUpdateInput};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 商户列表：默认仅未删除；`include_deleted=true` 返回含软删全量（交易筛选下拉用）。
#[tauri::command]
pub async fn list_merchants(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Merchant>> {
    let conn = db.conn.clone();
    run_db("list_merchants", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        merchant_domain::list_merchants(&conn, include_deleted.unwrap_or(false))
    })
    .await
}

/// 商户关联交易计数（issue #445，毛笔数口径）：每个商户（含软删）被未删流水引用的
/// 条数，实时推导、不落库，无引用商户计 0。商户管理列表专用读命令，纯读无写身份。
#[tauri::command]
pub async fn list_merchant_transaction_counts(
    db: State<'_, DbState>,
) -> Result<Vec<MerchantTransactionCount>> {
    let conn = db.conn.clone();
    run_db("list_merchant_transaction_counts", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        merchant_domain::transaction_counts(&conn)
    })
    .await
}

#[tauri::command]
pub async fn create_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: MerchantInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    let id = run_db("create_merchant", move || {
        crate::db::write(&conn, |conn| merchant_domain::create_merchant(conn, input))
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79 / ADR-0012）
    emit_for(&app, WriteOp::CreateMerchant, WriteEvidence::None);
    Ok(id)
}

#[tauri::command]
pub async fn update_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: MerchantUpdateInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("update_merchant", move || {
        crate::db::write(&conn, |conn| {
            merchant_domain::update_merchant(conn, &id, input)
        })
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044）
    emit_for(&app, WriteOp::UpdateMerchant, WriteEvidence::None);
    Ok(())
}

#[tauri::command]
pub async fn delete_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("delete_merchant", move || {
        crate::db::write(&conn, |conn| merchant_domain::delete_merchant(conn, &id))
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044）
    emit_for(&app, WriteOp::DeleteMerchant, WriteEvidence::None);
    Ok(())
}
