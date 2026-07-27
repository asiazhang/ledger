use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::<tauri::Wry>::new("log")
        .setup(|app, _api| {
            app.manage(LogState);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![open_log_dir])
        .build()
}

struct LogState;

#[tauri::command]
fn open_log_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = crate::logger::log_dir().to_string_lossy().to_string();
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|e| format!("打开日志目录失败: {e}"))?;
    Ok(())
}
