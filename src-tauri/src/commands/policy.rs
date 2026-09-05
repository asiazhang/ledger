//! IPC 命令壳 · 保单（Policy）（issue #360 / spec #358 / ADR-0051）：创建、列出、
//! 编辑、软删除保单与保单视角统计五个命令。
//!
//! 只做参数解包、事务壳与信号发射，不含业务语义；行为权威在
//! [`crate::policy`]（阶段 2 域目录化，#398 / ADR-0056）。
//!
//! 信号约定：保单是独立领域（ADR-0051），复用 `ledger:changed` 同名事件——
//! 保单 store 订阅后自动重拉。信号经统一写入口按写操作身份发射（ADR-0073）；
//! 域内 notify 参数保留为 BDD 计数注入点（ADR-0044 决策 8），生产壳层传空回调。
//!
//! 写命令经壳层统一写入口 [`crate::write_entry::write_entry`]（ADR-0073）：
//! 仪式（锁、事务、置脏、信号）内化单点；读命令经 `run_db`（形状乙）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::policy::{self as policy_domain, Policy, PolicyInput, PolicyStats};
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

#[tauri::command]
pub async fn list_policies(db: State<'_, DbState>) -> Result<Vec<Policy>> {
    let conn = db.conn.clone();
    run_db("list_policies", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        policy_domain::list_policies(&conn)
    })
    .await
}

/// 逐保单视角统计（issue #363）：只读聚合（先例 `subscription_spend_overview`），
/// today 注入本地今日，实时推导不落库、不发出失效信号。
#[tauri::command]
pub async fn list_policy_stats(db: State<'_, DbState>) -> Result<Vec<PolicyStats>> {
    let conn = db.conn.clone();
    run_db("list_policy_stats", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        policy_domain::policy_stats(&conn, chrono::Local::now().date_naive())
    })
    .await
}

#[tauri::command]
pub async fn create_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: PolicyInput,
) -> Result<String> {
    let conn = db.conn.clone();
    write_entry(
        "create_policy",
        conn,
        Some(&app),
        WriteOp::CreatePolicy,
        // notify 是 BDD 计数注入点（ADR-0044 决策 8）；信号已由写入口按身份
        // 在提交成功后发射，生产壳层传空回调。
        move |conn| policy_domain::create_policy(conn, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PolicyInput,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "update_policy",
        conn,
        Some(&app),
        WriteOp::UpdatePolicy,
        move |conn| policy_domain::update_policy(conn, &id, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_policy(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "delete_policy",
        conn,
        Some(&app),
        WriteOp::DeletePolicy,
        move |conn| policy_domain::delete_policy(conn, &id, &mut || {}).map(Outcome::Silent),
    )
    .await
}
