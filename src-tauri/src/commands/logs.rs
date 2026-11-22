use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn open_log_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = crate::shell_support::logger::log_dir()
        .to_string_lossy()
        .to_string();
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|e| format!("打开日志目录失败：{e}"))?;
    Ok(())
}

#[tauri::command]
pub fn log_frontend_error(message: String) {
    tracing::error!(target: "frontend_error", "前端渲染错误 message={}", message);
}

#[tauri::command]
pub fn reveal_in_file_manager(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| format!("在访达中显示失败：{e}"))?;
    Ok(())
}
