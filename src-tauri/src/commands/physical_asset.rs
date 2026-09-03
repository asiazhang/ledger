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

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::physical_asset::{
    self as physical_asset_domain, PhysicalAsset, PhysicalAssetDisposeInput, PhysicalAssetInput,
    PhysicalAssetList, PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

#[tauri::command]
pub fn list_physical_assets(
    db: State<'_, DbState>,
    status: Option<String>,
) -> Result<PhysicalAssetList> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    physical_asset_domain::list_physical_assets(&conn, status.as_deref())
}

#[tauri::command]
pub fn get_physical_asset(db: State<'_, DbState>, id: String) -> Result<PhysicalAsset> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    physical_asset_domain::get_physical_asset(&conn, &id)
}

#[tauri::command]
pub fn create_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: PhysicalAssetInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    // 域内自持事务保证资产行 + 首条估值行两表原子；域事务在闭包返回前已
    // 提交，`db.write` 的 is_autocommit 复核与置脏照常生效（ADR-0033 嵌套感知）。
    db.write(|conn| {
        physical_asset_domain::create_physical_asset(conn, input, &mut || {
            // 实物资产是独立领域（ADR-0064，同物品/保单先例）：复用
            // `ledger:changed` 同名事件。发不发由映射单点判定（ADR-0044）。
            emit_for(&app, WriteOp::CreatePhysicalAsset, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn update_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PhysicalAssetUpdateInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏；单表更新，域函数内无自持事务。
    db.write(|conn| {
        physical_asset_domain::update_physical_asset(conn, &id, input, &mut || {
            emit_for(&app, WriteOp::UpdatePhysicalAsset, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn dispose_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PhysicalAssetDisposeInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：状态标记 + 处置信息落库 + 置脏；单表更新。
    db.write(|conn| {
        physical_asset_domain::dispose_physical_asset(conn, &id, input, &mut || {
            emit_for(&app, WriteOp::DisposePhysicalAsset, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn delete_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：软删标志落库 + 置脏；单表更新。
    db.write(|conn| {
        physical_asset_domain::delete_physical_asset(conn, &id, &mut || {
            emit_for(&app, WriteOp::DeletePhysicalAsset, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn update_physical_asset_valuation(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: PhysicalAssetValuationInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：追加估值历史行 + 置脏；单表插入。
    db.write(|conn| {
        physical_asset_domain::update_physical_asset_valuation(conn, &id, input, &mut || {
            emit_for(
                &app,
                WriteOp::UpdatePhysicalAssetValuation,
                WriteEvidence::None,
            );
        })
    })
}
