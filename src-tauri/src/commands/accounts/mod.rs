//! 账户（issue #91）：命令外壳 + 核心逻辑 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `core`：核心逻辑——账户 CRUD/幂等创建/软删除/余额清单（原 accounts.rs 非命令部分，保持原状不拆分）；
//! - `tests`：原内嵌测试外迁。
//!
//! 对外仅暴露 `list_accounts` / `create_account` / `delete_account` /
//! `list_account_balances` 命令与 `*_internal` 复用函数（`commands/mod.rs` 经
//! `pub use accounts::*` 重导出，注册路径与前端/api_server/BDD 调用零改动）。

pub(crate) mod core; // 净资产聚合（dashboard）复用余额口径，需跨命令模块可见（issue #142）
#[cfg(test)]
mod tests;

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountUpdateInput,
};

pub use core::*;

#[tauri::command]
pub fn list_accounts(db: State<'_, DbState>) -> Result<Vec<Account>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    core::list_accounts_internal(&conn)
}

#[tauri::command]
pub fn create_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: AccountInput,
) -> Result<String> {
    let id = {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        core::create_account_internal(&conn, input)?
    };
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_reference_changed(&app, "create_account");
    Ok(id)
}

#[tauri::command]
pub fn delete_account(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        core::delete_account_internal(&conn, &id)?;
    }
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_reference_changed(&app, "delete_account");
    Ok(())
}

#[tauri::command]
pub fn update_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: AccountUpdateInput,
) -> Result<()> {
    {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        core::update_account_internal(&conn, &id, input)?;
    }
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_reference_changed(&app, "update_account");
    Ok(())
}

/// 余额调整（ADR-0026）：生成一笔与黑洞账户的转账，把余额校准到目标值。
/// 返回新交易 id；若按需新建了黑洞账户（参考表变更），发 `ledger:changed` 信号
/// （交易类写入本身不触发，与既有约定一致）。
#[tauri::command]
pub fn adjust_account_balance(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: AccountBalanceAdjustInput,
) -> Result<String> {
    let (tx_id, created_black_hole) = {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        core::adjust_account_balance_internal(&conn, &id, &input)?
    };
    if created_black_hole {
        // 按需新建黑洞账户 = 参考表变更 → 通知前端重拉（经 is_reference_write 白名单发射）
        crate::events::emit_reference_changed(&app, "create_account");
    }
    Ok(tx_id)
}

/// 批量查询所有账户余额，单次数据库往返完成。
#[tauri::command]
pub fn list_account_balances(db: State<'_, DbState>) -> Result<Vec<AccountBalance>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    core::list_account_balances_with_visibility(&conn, false)
}
