//! 商户（issue #188 / ADR-0028）：命令外壳 + 核心逻辑 + 内嵌测试。
//!
//! 目录组织：
//! - `core`：核心逻辑——列表 / 创建 / 更新（改名）/ 软删除 + 同名校验；
//! - `tests`：核心逻辑测试。
//!
//! 参考写入命令成功后经 [`crate::events::emit_reference_changed`] 发 `ledger:changed`
//! 失效信号（商户为第四张参考表，ADR-0012 / ADR-0028）；命令名已同步登记进
//! `events::REFERENCE_WRITE_COMMANDS`。
//! 对外暴露的命令与 `*_internal` 复用函数经 `commands/mod.rs` 的 `pub use merchants::*`
//! 重导出，注册路径与前端调用零改动。

mod core;
#[cfg(test)]
mod tests;

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{Merchant, MerchantInput, MerchantUpdateInput};

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
    let id = {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        core::create_merchant_internal(&conn, input)?
    };
    // 参考写入成功 → 通知前端重拉参考数据（issue #79 / ADR-0012）
    crate::events::emit_reference_changed(&app, "create_merchant");
    Ok(id)
}

#[tauri::command]
pub fn update_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: MerchantUpdateInput,
) -> Result<()> {
    {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        core::update_merchant_internal(&conn, &id, input)?;
    }
    // 参考写入成功 → 通知前端重拉参考数据
    crate::events::emit_reference_changed(&app, "update_merchant");
    Ok(())
}

#[tauri::command]
pub fn delete_merchant(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        core::delete_merchant_internal(&conn, &id)?;
    }
    // 参考写入成功 → 通知前端重拉参考数据
    crate::events::emit_reference_changed(&app, "delete_merchant");
    Ok(())
}
