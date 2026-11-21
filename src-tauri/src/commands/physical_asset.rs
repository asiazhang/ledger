#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;
use ledger_physical_asset::{
    self as physical_asset_domain, PhysicalAsset, PhysicalAssetDisposeInput, PhysicalAssetInput,
    PhysicalAssetList, PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
};

#[tauri::command]
pub async fn list_physical_assets(
    db: State<'_, DbState>,
    status: Option<String>,
) -> Result<PhysicalAssetList> {
    let conn = db.read_handle();
    read_entry("list_physical_assets", conn, move |conn| {
        physical_asset_domain::list_physical_assets(conn, status.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn get_physical_asset(db: State<'_, DbState>, id: String) -> Result<PhysicalAsset> {
    let conn = db.read_handle();
    read_entry("get_physical_asset", conn, move |conn| {
        physical_asset_domain::get_physical_asset(conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn create_physical_asset(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: PhysicalAssetInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_physical_asset",
        conn,
        Some(&app),
        WriteOp::CreatePhysicalAsset,
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
    let conn = db.write_handle();
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
    let conn = db.write_handle();
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
    let conn = db.write_handle();
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
    let conn = db.write_handle();
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
