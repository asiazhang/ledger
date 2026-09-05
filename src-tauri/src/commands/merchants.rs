//! IPC 命令壳 · 商户（Merchant）。
//!
//! 只负责参数解包与统一写入口一行调用；商户字典行为位于 [`crate::merchants`]。
//! 注册路径与前端调用保持不变。
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
use crate::merchants as merchant_domain;
use crate::merchants::{Merchant, MerchantInput, MerchantTransactionCount, MerchantUpdateInput};
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

/// 商户列表：默认仅未删除；`include_deleted=true` 返回含软删全量（交易筛选下拉用）。
#[tauri::command]
pub async fn list_merchants(
    db: State<'_, DbState>,
    include_deleted: Option<bool>,
) -> Result<Vec<Merchant>> {
    let conn = db.conn.clone();
    run_db("list_merchants", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        merchant_domain::list_merchants(&conn, include_deleted.unwrap_or(false))
    })
    .await
}

/// 商户关联交易计数（issue #445，毛笔数口径）：每个商户（含软删）被未删流水引用的
/// 条数，实时推导、不落库，无引用商户计 0。商户管理列表专用读命令，纯读无写身份。
#[tauri::command]
pub async fn list_merchant_transaction_counts(
    db: State<'_, DbState>,
) -> Result<Vec<MerchantTransactionCount>> {
    let conn = db.conn.clone();
    run_db("list_merchant_transaction_counts", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        merchant_domain::transaction_counts(&conn)
    })
    .await
}

#[tauri::command]
pub async fn create_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: MerchantInput,
) -> Result<String> {
    let conn = db.conn.clone();
    write_entry(
        "create_merchant",
        conn,
        Some(&app),
        WriteOp::CreateMerchant,
        move |conn| merchant_domain::create_merchant(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: MerchantUpdateInput,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "update_merchant",
        conn,
        Some(&app),
        WriteOp::UpdateMerchant,
        move |conn| merchant_domain::update_merchant(conn, &id, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_merchant(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "delete_merchant",
        conn,
        Some(&app),
        WriteOp::DeleteMerchant,
        move |conn| merchant_domain::delete_merchant(conn, &id).map(Outcome::Silent),
    )
    .await
}
