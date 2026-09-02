//! 账户（issue #91）：命令外壳 + 核心逻辑 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `core`：核心逻辑——账户 CRUD/幂等创建/软删除/余额清单（原 accounts.rs 非命令部分，保持原状不拆分）；
//! - `tests`：原内嵌测试外迁。
//!
//! 写命令成功后的失效信号经信号映射单点（`signals::emit_for`，ADR-0044）判定发射，
//! 壳层不持有「谁发什么」的判定知识；余额调整的条件信号由「黑洞即建」证据承载。
//!
//! 对外仅暴露 `list_accounts` / `create_account` / `delete_account` /
//! `adjust_account_balance` / `list_account_balances` 命令与 `*_internal` 复用函数
//! （`commands/mod.rs` 经 `pub use accounts::*` 重导出，注册路径与前端/api_server/BDD
//! 调用零改动）。

pub(crate) mod core; // 净资产聚合（dashboard）复用余额口径，需跨命令模块可见（issue #142）
#[cfg(test)]
mod tests;

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountUpdateInput,
};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

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
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = db.write(|conn| core::create_account_internal(conn, input))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::CreateAccount, WriteEvidence::None);
    Ok(id)
}

#[tauri::command]
pub fn delete_account(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| core::delete_account_internal(conn, &id))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::DeleteAccount, WriteEvidence::None);
    Ok(())
}

#[tauri::command]
pub fn update_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: AccountUpdateInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| core::update_account_internal(conn, &id, input))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::UpdateAccount, WriteEvidence::None);
    Ok(())
}

/// 余额调整（ADR-0026）：生成一笔与黑洞账户的转账，把余额校准到目标值。
/// 返回新交易 id；仅「按需新建黑洞账户」（参考表变更）发 `ledger:changed` 信号，
/// 纯转账（黑洞账户已存在）零信号——条件由「黑洞即建」证据承载，判定收在映射单点
/// （ADR-0044 决策 4）；交易类写入本身不触发，与既有约定一致。
#[tauri::command]
pub fn adjust_account_balance(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: AccountBalanceAdjustInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：核心逻辑自管事务（BEGIN/COMMIT/ROLLBACK），
    // 提交点置脏/到期检查由写入口在 `is_autocommit()` 复核时单点承接。
    let (tx_id, created_black_hole) =
        db.write(|conn| core::adjust_account_balance_internal(conn, &id, &input))?;
    // 按需新建黑洞账户 = 参考表变更 → 参考失效信号；纯转账零信号
    //（发不发由映射单点依证据判定，ADR-0044）。
    emit_for(
        &app,
        WriteOp::AdjustAccountBalance,
        WriteEvidence::BlackHoleCreated(created_black_hole),
    );
    Ok(tx_id)
}

/// 批量查询所有账户余额，单次数据库往返完成。
#[tauri::command]
pub fn list_account_balances(db: State<'_, DbState>) -> Result<Vec<AccountBalance>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::db::balance::list_account_balances_with_visibility(&conn, false)
}
