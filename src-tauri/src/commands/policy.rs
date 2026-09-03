//! IPC 命令壳 · 保单（Policy）（issue #360 / spec #358 / ADR-0051）：创建、列出、
//! 编辑、软删除保单与保单视角统计五个命令。
//!
//! 只做参数解包、事务壳与信号发射，不含业务语义；行为权威在
//! [`crate::policy`]（阶段 2 域目录化，#398 / ADR-0056）。
//!
//! 信号约定：保单是独立领域（ADR-0051），复用 `ledger:changed` 同名事件——
//! 保单 store 订阅后自动重拉。发不发、发哪个由映射单点判定
//! （ADR-0044 决策 8），notify 只是发射钩子。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：写路径对备份域
//! 零感知，置脏/到期检查由写入口闭包在提交点单点执行。

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::policy::{self as policy_domain, Policy, PolicyInput, PolicyStats};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

#[tauri::command]
pub fn list_policies(db: State<'_, DbState>) -> Result<Vec<Policy>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    policy_domain::list_policies(&conn)
}

/// 逐保单视角统计（issue #363）：只读聚合（先例 `subscription_spend_overview`），
/// today 注入本地今日，实时推导不落库、不发出失效信号。
#[tauri::command]
pub fn list_policy_stats(db: State<'_, DbState>) -> Result<Vec<PolicyStats>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    policy_domain::policy_stats(&conn, chrono::Local::now().date_naive())
}

#[tauri::command]
pub fn create_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: PolicyInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        policy_domain::create_policy(conn, input, &mut || {
            // 保单是独立领域（ADR-0051，同物品先例）：复用 `ledger:changed` 同名
            // 事件，保单 store 订阅后自动重拉。发不发由映射单点判定（ADR-0044）。
            emit_for(&app, WriteOp::CreatePolicy, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn update_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PolicyInput,
) -> Result<()> {
    db.write(|conn| {
        policy_domain::update_policy(conn, &id, input, &mut || {
            emit_for(&app, WriteOp::UpdatePolicy, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn delete_policy(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    db.write(|conn| {
        policy_domain::delete_policy(conn, &id, &mut || {
            emit_for(&app, WriteOp::DeletePolicy, WriteEvidence::None);
        })
    })
}
