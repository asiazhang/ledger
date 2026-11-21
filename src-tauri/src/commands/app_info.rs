use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppBuildInfo {
    pub version: String,
    pub platform: String,
    pub arch: String,
}

#[tauri::command]
pub fn app_build_info() -> AppBuildInfo {
    AppBuildInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}
