//! IPC 命令壳 · 账户（Account）。
//!
//! 只负责参数解包、事务边界与失效信号发射；账户域行为位于 [`crate::accounts`]。
//! 注册路径与前端调用保持不变。

use tauri::State;

use crate::accounts as account_domain;
use crate::accounts::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountUpdateInput,
};
use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 账户列表：默认仅未删除、不含隐藏账户（黑洞账户经 AI 侧端点/`*_for_api` 口径可见）。
#[tauri::command]
pub fn list_accounts(db: State<'_, DbState>) -> Result<Vec<Account>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    account_domain::list_accounts(&conn)
}

#[tauri::command]
pub fn create_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: AccountInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = db.write(|conn| account_domain::create_account(conn, input))?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::CreateAccount, WriteEvidence::None);
    Ok(id)
}

#[tauri::command]
pub fn delete_account(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| account_domain::delete_account(conn, &id))?;
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
    db.write(|conn| account_domain::update_account(conn, &id, input))?;
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
        db.write(|conn| account_domain::adjust_account_balance(conn, &id, &input))?;
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
    account_domain::list_account_balances_with_visibility(&conn, false)
}
