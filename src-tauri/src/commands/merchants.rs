//! IPC 命令壳 · 商户（Merchant）。
//!
//! 只负责参数解包、事务边界与失效信号发射；商户字典行为位于 [`crate::merchants`]。
//! 注册路径与前端调用保持不变。

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::merchants as merchant_domain;
use crate::models::{Merchant, MerchantInput, MerchantUpdateInput};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 商户列表：默认仅未删除；`include_deleted=true` 返回含软删全量（交易筛选下拉用）。
#[tauri::command]
pub fn list_merchants(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Merchant>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    merchant_domain::list_merchants(&conn, include_deleted.unwrap_or(false))
}

#[tauri::command]
pub fn create_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: MerchantInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = db.write(|conn| merchant_domain::create_merchant(conn, input))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79 / ADR-0012）
    emit_for(&app, WriteOp::CreateMerchant, WriteEvidence::None);
    Ok(id)
}

#[tauri::command]
pub fn update_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: MerchantUpdateInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| merchant_domain::update_merchant(conn, &id, input))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044）
    emit_for(&app, WriteOp::UpdateMerchant, WriteEvidence::None);
    Ok(())
}

#[tauri::command]
pub fn delete_merchant(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| merchant_domain::delete_merchant(conn, &id))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044）
    emit_for(&app, WriteOp::DeleteMerchant, WriteEvidence::None);
    Ok(())
}
