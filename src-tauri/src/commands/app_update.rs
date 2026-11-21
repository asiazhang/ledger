use tauri::AppHandle;
use tauri::Manager;

use ledger_infra::error::Result;

use crate::app_update::{self, AutoUpdateSwitch, UpdateCheckOutcome, UpdateDownloadOutcome};

#[tauri::command]
pub async fn check_for_updates<R: tauri::Runtime>(app: AppHandle<R>) -> Result<UpdateCheckOutcome> {
    #[cfg(desktop)]
    {
        app_update::check_update(&app, None).await
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err(app_update::unsupported_platform_error())
    }
}

#[tauri::command]
pub async fn download_update<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<UpdateDownloadOutcome> {
    #[cfg(desktop)]
    {
        app_update::download_update(&app, None).await
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err(app_update::unsupported_platform_error())
    }
}

#[tauri::command]
pub async fn install_update<R: tauri::Runtime>(app: AppHandle<R>) -> Result<()> {
    #[cfg(desktop)]
    {
        app_update::install_pending_update(&app).await
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err(app_update::unsupported_platform_error())
    }
}

#[tauri::command]
pub fn set_auto_update_enabled<R: tauri::Runtime>(app: AppHandle<R>, enabled: bool) -> Result<()> {
    app.state::<AutoUpdateSwitch>().set(enabled);
    tracing::info!(enabled, "自动检查更新开关镜像已更新");
    Ok(())
}

#[tauri::command]
pub fn get_update_surface() -> Result<app_update::UpdateSurface> {
    Ok(app_update::update_surface(
        std::env::var_os("APPIMAGE").as_deref(),
    ))
}
