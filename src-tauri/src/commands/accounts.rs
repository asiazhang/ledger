//! IPC 命令壳 · 账户（Account）。
//!
//! 只负责参数解包与统一写入口一行调用；账户域行为位于 [`crate::accounts`]。
//! 注册路径与前端调用保持不变。
//!
//! 全部命令 async 化（形状乙，spec #498 / #501）；写命令经壳层统一写入口
//! [`crate::write_entry::write_entry`]（ADR-0073）：连接、发射器、写操作身份、
//! 业务闭包进，仪式（锁、事务、置脏、信号）内化单点。
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
use crate::signals::{WriteEvidence, WriteOp};
use crate::write_entry::{Outcome, write_entry};

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
    // 壳层统一写入口（ADR-0073）：置脏、信号内化单点，参考写入成功发参考失效信号。
    let conn = db.conn.clone();
    write_entry(
        "create_account",
        conn,
        Some(&app),
        WriteOp::CreateAccount,
        move |conn| account_domain::create_account(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "delete_account",
        conn,
        Some(&app),
        WriteOp::DeleteAccount,
        move |conn| account_domain::delete_account(conn, &id).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_account(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: AccountUpdateInput,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "update_account",
        conn,
        Some(&app),
        WriteOp::UpdateAccount,
        move |conn| account_domain::update_account(conn, &id, input).map(Outcome::Silent),
    )
    .await
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
    // 核心逻辑自管事务（BEGIN/COMMIT/ROLLBACK），提交点置脏/到期检查由写入口
    // 在 `is_autocommit()` 复核时单点承接；黑洞即建证据随闭包返回必达。
    let conn = db.conn.clone();
    write_entry(
        "adjust_account_balance",
        conn,
        Some(&app),
        WriteOp::AdjustAccountBalance,
        move |conn| {
            account_domain::adjust_account_balance(conn, &id, &input).map(|(tx_id, created)| {
                Outcome::Evidenced(tx_id, WriteEvidence::BlackHoleCreated(created))
            })
        },
    )
    .await
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
