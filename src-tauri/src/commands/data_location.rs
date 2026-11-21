#![allow(clippy::unreachable)]

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, Runtime};

use crate::commands::boot::current_boot;
use ledger_infra::db::data_location;
use ledger_infra::db::run_db;
use ledger_infra::error::{AppError, Result};

pub(crate) fn default_data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Io(format!("获取默认数据目录失败：{e}")))
}

pub(crate) fn effective_db_dir_of<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let default_dir = default_data_dir(app)?;
    let boot = current_boot(app);
    Ok(data_location::effective_db_dir(boot.as_ref(), &default_dir))
}

#[tauri::command]
pub async fn get_data_location_info(app: AppHandle) -> Result<data_location::DataLocationInfo> {
    run_db("get_data_location_info", move || {
        let default_dir = default_data_dir(&app)?;
        let boot = current_boot(&app);
        Ok(data_location::gather_info_from_boot(
            &default_dir,
            boot.as_ref(),
        ))
    })
    .await
}

#[tauri::command]
pub async fn submit_data_location_change(
    app: AppHandle,
    target_dir: String,
    adopt_existing: bool,
) -> Result<data_location::DataLocationChangeOutcome> {
    run_db("submit_data_location_change", move || {
        data_location::ensure_relocation_settled(current_boot(&app).as_ref())?;
        let default_dir = default_data_dir(&app)?;
        let trimmed = target_dir.trim();
        if trimmed.is_empty() {
            return Err(AppError::coded(
                "data-location.dir-required",
                "目标目录不能为空",
            ));
        }
        data_location::validate_and_commit(&default_dir, Path::new(trimmed), adopt_existing)
    })
    .await
}

#[tauri::command]
pub async fn restore_default_data_location(
    app: AppHandle,
    adopt_existing: bool,
) -> Result<data_location::DataLocationChangeOutcome> {
    run_db("restore_default_data_location", move || {
        data_location::ensure_relocation_settled(current_boot(&app).as_ref())?;
        let default_dir = default_data_dir(&app)?;
        data_location::validate_and_commit(&default_dir, &default_dir, adopt_existing)
    })
    .await
}
