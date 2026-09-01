//! 商户（issue #188 / ADR-0028）：命令外壳 + 核心逻辑 + 内嵌测试。
//!
//! 目录组织：
//! - `core`：核心逻辑——列表 / 创建 / 更新（改名）/ 软删除 + 同名校验；
//! - `tests`：核心逻辑测试。
//!
//! 参考写命令成功后的失效信号经信号映射单点（[`crate::signals::emit_for`]）判定
//! 发 `ledger:changed`（商户为第四张参考表，ADR-0012 / ADR-0028；ADR-0044）。
//! 对外暴露的命令与 `*_internal` 复用函数经 `commands/mod.rs` 的 `pub use merchants::*`
//! 重导出，注册路径与前端调用零改动。

mod core;
#[cfg(test)]
mod tests;

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{Merchant, MerchantInput, MerchantUpdateInput};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

pub use core::*;

/// 商户列表：默认仅未删除；`include_deleted=true` 返回含软删全量（交易筛选下拉用）。
#[tauri::command]
pub fn list_merchants(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Merchant>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    core::list_merchants_internal(&conn, include_deleted.unwrap_or(false))
}

#[tauri::command]
pub fn create_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: MerchantInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = db.write(|conn| core::create_merchant_internal(conn, input))?;
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
    db.write(|conn| core::update_merchant_internal(conn, &id, input))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044）
    emit_for(&app, WriteOp::UpdateMerchant, WriteEvidence::None);
    Ok(())
}

#[tauri::command]
pub fn delete_merchant(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| core::delete_merchant_internal(conn, &id))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044）
    emit_for(&app, WriteOp::DeleteMerchant, WriteEvidence::None);
    Ok(())
}
