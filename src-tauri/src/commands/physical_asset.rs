//! IPC 命令壳 · 实物资产（PhysicalAsset）（issue #466 / spec #465 / ADR-0064）：
//! 建档、列表（含在持合计与状态筛选参数）与详情三个命令（T1），编辑档案与
//! 更新估值两个命令（T2，issue #467）。
//!
//! 只做参数解包、事务壳与信号发射，不含业务语义；行为权威在
//! [`crate::physical_asset`]（ADR-0056 分层）。
//!
//! 信号约定：实物资产是独立领域（ADR-0064），复用 `ledger:changed` 同名
//! 事件——实物资产 store 订阅后自动重拉。信号经统一写入口按写操作身份发射
//! （ADR-0073）；域内 notify 参数保留为 BDD 计数注入点（ADR-0044 决策 8），
//! 生产壳层传空回调。
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
use crate::physical_asset::{
    self as physical_asset_domain, PhysicalAsset, PhysicalAssetDisposeInput, PhysicalAssetInput,
    PhysicalAssetList, PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
};
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

#[tauri::command]
pub async fn list_physical_assets(
    db: State<'_, DbState>,
    status: Option<String>,
) -> Result<PhysicalAssetList> {
    let conn = db.conn.clone();
    run_db("list_physical_assets", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        physical_asset_domain::list_physical_assets(&conn, status.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn get_physical_asset(db: State<'_, DbState>, id: String) -> Result<PhysicalAsset> {
    let conn = db.conn.clone();
    run_db("get_physical_asset", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        physical_asset_domain::get_physical_asset(&conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn create_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: PhysicalAssetInput,
) -> Result<String> {
    // 域内自持事务保证资产行 + 首条估值行两表原子；域事务在闭包返回前已
    // 提交，写入口的 is_autocommit 复核与置脏照常生效（ADR-0033 嵌套感知）。
    let conn = db.conn.clone();
    write_entry(
        "create_physical_asset",
        conn,
        Some(&app),
        WriteOp::CreatePhysicalAsset,
        // notify 是 BDD 计数注入点（ADR-0044 决策 8）；信号已由写入口按身份
        // 在提交成功后发射，生产壳层传空回调。
        move |conn| {
            physical_asset_domain::create_physical_asset(conn, input, &mut || {})
                .map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn update_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PhysicalAssetUpdateInput,
) -> Result<()> {
    // 单表更新，域函数内无自持事务。
    let conn = db.conn.clone();
    write_entry(
        "update_physical_asset",
        conn,
        Some(&app),
        WriteOp::UpdatePhysicalAsset,
        move |conn| {
            physical_asset_domain::update_physical_asset(conn, &id, input, &mut || {})
                .map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn dispose_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PhysicalAssetDisposeInput,
) -> Result<()> {
    // 状态标记 + 处置信息落库；单表更新。
    let conn = db.conn.clone();
    write_entry(
        "dispose_physical_asset",
        conn,
        Some(&app),
        WriteOp::DisposePhysicalAsset,
        move |conn| {
            physical_asset_domain::dispose_physical_asset(conn, &id, input, &mut || {})
                .map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn delete_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    // 软删标志落库；单表更新。
    let conn = db.conn.clone();
    write_entry(
        "delete_physical_asset",
        conn,
        Some(&app),
        WriteOp::DeletePhysicalAsset,
        move |conn| {
            physical_asset_domain::delete_physical_asset(conn, &id, &mut || {}).map(Outcome::Silent)
        },
    )
    .await
}

#[tauri::command]
pub async fn update_physical_asset_valuation(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PhysicalAssetValuationInput,
) -> Result<()> {
    // 追加估值历史行；单表插入。
    let conn = db.conn.clone();
    write_entry(
        "update_physical_asset_valuation",
        conn,
        Some(&app),
        WriteOp::UpdatePhysicalAssetValuation,
        move |conn| {
            physical_asset_domain::update_physical_asset_valuation(conn, &id, input, &mut || {})
                .map(Outcome::Silent)
        },
    )
    .await
}
