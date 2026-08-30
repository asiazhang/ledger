use tauri_plugin_opener::OpenerExt;

/// 打开日志目录（issue #283）：经系统文件管理器展示日志文件
/// （按天滚动、保留 7 天，目录解析复用 [`crate::logger`] 既有语义）。
/// 错误以「打开日志目录失败：」中文前缀返回，前端原样透传即得可读提示。
#[tauri::command]
pub fn open_log_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = crate::logger::log_dir().to_string_lossy().to_string();
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|e| format!("打开日志目录失败：{e}"))?;
    Ok(())
}
