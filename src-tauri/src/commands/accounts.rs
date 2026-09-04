//! IPC 命令壳 · 账户（Account）。
//!
//! 只负责参数解包、事务边界与失效信号发射；账户域行为位于 [`crate::accounts`]。
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

use crate::accounts as account_domain;
use crate::accounts::{
    Account, AccountBalance, AccountBalanceAdjustInput, AccountInput, AccountUpdateInput,
    BalanceCacheAudit,
};
use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

/// 账户列表：默认仅未删除、不含隐藏账户（黑洞账户经 AI 侧端点/`*_for_api` 口径可见）。
#[tauri::command]
pub async fn list_accounts(db: State<'_, DbState>) -> Result<Vec<Account>> {
    let conn = db.conn.clone();
    run_db("list_accounts", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        account_domain::list_accounts(&conn)
    })
    .await
}

#[tauri::command]
pub async fn create_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: AccountInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    let id = run_db("create_account", move || {
        crate::db::write(&conn, |conn| account_domain::create_account(conn, input))
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::CreateAccount, WriteEvidence::None);
    Ok(id)
}

#[tauri::command]
pub async fn delete_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("delete_account", move || {
        crate::db::write(&conn, |conn| account_domain::delete_account(conn, &id))
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::DeleteAccount, WriteEvidence::None);
    Ok(())
}

#[tauri::command]
pub async fn update_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: AccountUpdateInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("update_account", move || {
        crate::db::write(&conn, |conn| {
            account_domain::update_account(conn, &id, input)
        })
    })
    .await?;
    // 参考写入成功 → 参考失效信号（映射单点判定，ADR-0044；issue #79）
    emit_for(&app, WriteOp::UpdateAccount, WriteEvidence::None);
    Ok(())
}

/// 余额调整（ADR-0026）：生成一笔与黑洞账户的转账，把余额校准到目标值。
/// 返回新交易 id；仅「按需新建黑洞账户」（参考表变更）发 `ledger:changed` 信号，
/// 纯转账（黑洞账户已存在）零信号——条件由「黑洞即建」证据承载，判定收在映射单点
/// （ADR-0044 决策 4）；交易类写入本身不触发，与既有约定一致。
#[tauri::command]
pub async fn adjust_account_balance(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: AccountBalanceAdjustInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：核心逻辑自管事务（BEGIN/COMMIT/ROLLBACK），
    // 提交点置脏/到期检查由写入口在 `is_autocommit()` 复核时单点承接。
    let conn = db.conn.clone();
    let (tx_id, created_black_hole) = run_db("adjust_account_balance", move || {
        crate::db::write(&conn, |conn| {
            account_domain::adjust_account_balance(conn, &id, &input)
        })
    })
    .await?;
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
pub async fn list_account_balances(db: State<'_, DbState>) -> Result<Vec<AccountBalance>> {
    let conn = db.conn.clone();
    run_db("list_account_balances", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        account_domain::list_account_balances_with_visibility(&conn, false)
    })
    .await
}

/// 手动审计命令（issue #491 / ADR-0067，唯一新接缝）：全账户实时重算 vs 余额缓存，
/// 修复差异并返回差异报告。缓存属派生数据：修复不置脏、不发信号（领域层说明），
/// 故不经 `db.write` 包装，直连锁内执行。
#[tauri::command]
pub async fn audit_balance_cache(db: State<'_, DbState>) -> Result<BalanceCacheAudit> {
    let conn = db.conn.clone();
    run_db("audit_balance_cache", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        account_domain::audit_balance_cache(&conn)
    })
    .await
}
