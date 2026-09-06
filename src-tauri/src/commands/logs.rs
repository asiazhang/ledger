//! 系统 opener 命令壳：「打开日志目录」（issue #283）与「在访达中显示」（issue #653）。
//!
//! 纯壳系统命令，无业务语义（ADR-0056：确认纯壳不建域）。保持同步形态（形状乙
//! sweep 判定，spec #498 / #503）：不触 DB，opener 调用是即发即忘的系统调用
//! （请求系统文件管理器后即刻返回），无阻塞工作面；且错误契约是
//! `Result<(), String>` 中文前缀原文——改走 async helper 需把错误归一为
//! `AppError`，会改变 IPC 错误载荷形状，得不偿失。
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

/// 在系统文件管理器中定位指定文件（issue #653）：经系统 opener 插件的文件定位
/// 能力在文件管理器中选中该文件（macOS 为访达、Windows 为资源管理器，按平台
/// 映射）。入参是任意路径——命令本身不持有备份语义，消费方决定传什么。
/// 错误以「在访达中显示失败：」中文前缀返回，前端原样透传即得可读提示。
#[tauri::command]
pub fn reveal_in_file_manager(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| format!("在访达中显示失败：{e}"))?;
    Ok(())
}
