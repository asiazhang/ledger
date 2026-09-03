//! DataLocation 领域命令壳层（issue #133 / ADR-0018；#408 压平为纯壳）。
//!
//! 只做参数解包与 [`crate::db::data_location`] 调用：三步校验与信息聚合的
//! 业务逻辑已下沉 db 基础设施，本文件不含领域规则。「更改位置」的三步校验
//! 语义（① 自动创建目录、② 试写探针、③ 既有库二选一）见
//! [`data_location::validate_and_commit`]；校验通过后意图写入指针文件，
//! 真实搬迁只发生在下次启动（引导内核完成）。保持 ADR-0017 的领域形状约定：
//! 单个类型化 DTO、不做通用 KV 透传。

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::db::data_location;
use crate::error::{AppError, Result};

/// 默认应用数据目录（指针文件所在地，也是「恢复默认」的目标）。
fn default_data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Io(format!("获取默认数据目录失败：{e}")))
}

/// 获取 DataLocation 信息：当前生效目录、是否已更改待重启生效、回退警示。
#[tauri::command]
pub fn get_data_location_info(app: AppHandle) -> Result<data_location::DataLocationInfo> {
    let default_dir = default_data_dir(&app)?;
    let boot = app
        .try_state::<data_location::Boot>()
        .map(|state| state.inner().clone());
    Ok(data_location::gather_info_from_boot(
        &default_dir,
        boot.as_ref(),
    ))
}

/// 提交更改位置意图：三步校验通过后写入指针文件，下次启动搬迁生效。
#[tauri::command]
pub fn submit_data_location_change(
    app: AppHandle,
    target_dir: String,
    adopt_existing: bool,
) -> Result<data_location::DataLocationChangeOutcome> {
    let default_dir = default_data_dir(&app)?;
    let trimmed = target_dir.trim();
    if trimmed.is_empty() {
        return Err(AppError::coded(
            "data-location.dir-required",
            "目标目录不能为空",
        ));
    }
    data_location::validate_and_commit(&default_dir, Path::new(trimmed), adopt_existing)
}

/// 恢复默认位置：与更改完全相同的校验 + 写意图机制，目标是默认应用数据目录。
/// 默认目录可能仍保留搬迁前的旧库（原库永久保留），此时同样返回二选一信号。
#[tauri::command]
pub fn restore_default_data_location(
    app: AppHandle,
    adopt_existing: bool,
) -> Result<data_location::DataLocationChangeOutcome> {
    let default_dir = default_data_dir(&app)?;
    data_location::validate_and_commit(&default_dir, &default_dir, adopt_existing)
}
