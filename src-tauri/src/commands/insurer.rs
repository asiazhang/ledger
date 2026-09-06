//! IPC 命令壳 · 保司（Insurer，issue #712 / ADR-0082）。
//!
//! 只负责参数解包与统一写入口一行调用；保司字典行为位于 [`crate::policy::insurer`]。
//!
//! 全部命令 async 化（形状乙，spec #498 / #501）；写命令经壳层统一写入口
//! [`crate::write_entry::write_entry`]（ADR-0073）：仪式内化单点，参考写入成功
//! 发参考失效信号（映射单点判定，ADR-0044）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::policy as policy_domain;
use crate::policy::{Insurer, InsurerInput, InsurerUpdateInput};
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

/// 保司列表：默认仅未删除；`include_deleted=true` 返回含软删全量
/// （保司管理「显示已删」切换用，issue #714 消费）。
#[tauri::command]
pub async fn list_insurers(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Insurer>> {
    let conn = db.conn.clone();
    run_db("list_insurers", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        policy_domain::list_insurers(&conn, include_deleted.unwrap_or(false))
    })
    .await
}

#[tauri::command]
pub async fn create_insurer(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: InsurerInput,
) -> Result<String> {
    let conn = db.conn.clone();
    write_entry(
        "create_insurer",
        conn,
        Some(&app),
        WriteOp::CreateInsurer,
        move |conn| policy_domain::create_insurer(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_insurer(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: InsurerUpdateInput,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "update_insurer",
        conn,
        Some(&app),
        WriteOp::UpdateInsurer,
        move |conn| policy_domain::update_insurer(conn, &id, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_insurer(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "delete_insurer",
        conn,
        Some(&app),
        WriteOp::DeleteInsurer,
        move |conn| policy_domain::delete_insurer(conn, &id).map(Outcome::Silent),
    )
    .await
}
