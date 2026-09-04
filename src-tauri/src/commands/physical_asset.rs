//! IPC 命令壳 · 实物资产（PhysicalAsset）（issue #466 / spec #465 / ADR-0064）：
//! 建档、列表（含在持合计与状态筛选参数）与详情三个命令（T1），编辑档案与
//! 更新估值两个命令（T2，issue #467）。
//!
//! 只做参数解包、事务壳与信号发射，不含业务语义；行为权威在
//! [`crate::physical_asset`]（ADR-0056 分层）。
//!
//! 信号约定：实物资产是独立领域（ADR-0064），复用 `ledger:changed` 同名
//! 事件——实物资产 store 订阅后自动重拉。发不发、发哪个由映射单点判定
//! （ADR-0044），notify 只是发射钩子。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）。
//!
//! 全部命令 async 化（形状乙，spec #498 / #502）：DB 调用经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程；
//! 写路径仍在连接层统一写入口内置脏（ADR-0032 语义零改动）。notify 回调里的
//! 信号发射经 `post_emit_with` 投递主线程队尾（ADR-0054 主线程非阻塞投递），
//! 从阻塞线程调用安全，对用户外部行为不变。
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
use crate::signals::{WriteEvidence, WriteOp, emit_for};

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
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    // 域内自持事务保证资产行 + 首条估值行两表原子；域事务在闭包返回前已
    // 提交，`db.write` 的 is_autocommit 复核与置脏照常生效（ADR-0033 嵌套感知）。
    let conn = db.conn.clone();
    run_db("create_physical_asset", move || {
        crate::db::write(&conn, |conn| {
            physical_asset_domain::create_physical_asset(conn, input, &mut || {
                // 实物资产是独立领域（ADR-0064，同物品/保单先例）：复用
                // `ledger:changed` 同名事件。发不发由映射单点判定（ADR-0044）。
                emit_for(&app, WriteOp::CreatePhysicalAsset, WriteEvidence::None);
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn update_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PhysicalAssetUpdateInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏；单表更新，域函数内无自持事务。
    let conn = db.conn.clone();
    run_db("update_physical_asset", move || {
        crate::db::write(&conn, |conn| {
            physical_asset_domain::update_physical_asset(conn, &id, input, &mut || {
                emit_for(&app, WriteOp::UpdatePhysicalAsset, WriteEvidence::None);
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn dispose_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PhysicalAssetDisposeInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：状态标记 + 处置信息落库 + 置脏；单表更新。
    let conn = db.conn.clone();
    run_db("dispose_physical_asset", move || {
        crate::db::write(&conn, |conn| {
            physical_asset_domain::dispose_physical_asset(conn, &id, input, &mut || {
                emit_for(&app, WriteOp::DisposePhysicalAsset, WriteEvidence::None);
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn delete_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：软删标志落库 + 置脏；单表更新。
    let conn = db.conn.clone();
    run_db("delete_physical_asset", move || {
        crate::db::write(&conn, |conn| {
            physical_asset_domain::delete_physical_asset(conn, &id, &mut || {
                emit_for(&app, WriteOp::DeletePhysicalAsset, WriteEvidence::None);
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn update_physical_asset_valuation(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PhysicalAssetValuationInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：追加估值历史行 + 置脏；单表插入。
    let conn = db.conn.clone();
    run_db("update_physical_asset_valuation", move || {
        crate::db::write(&conn, |conn| {
            physical_asset_domain::update_physical_asset_valuation(conn, &id, input, &mut || {
                emit_for(
                    &app,
                    WriteOp::UpdatePhysicalAssetValuation,
                    WriteEvidence::None,
                );
            })
        })
    })
    .await
}
