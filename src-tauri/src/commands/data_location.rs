//! DataLocation 领域命令壳层（issue #133 / ADR-0018；#408 压平为纯壳）。
//!
//! 只做参数解包与 [`crate::db::data_location`] 调用：三步校验与信息聚合的
//! 业务逻辑已下沉 db 基础设施，本文件不含领域规则。「更改位置」的三步校验
//! 语义（① 自动创建目录、② 试写探针、③ 既有库二选一）见
//! [`data_location::validate_and_commit`]；校验通过后意图写入指针文件，
//! 真实搬迁只发生在下次启动（引导内核完成）。保持 ADR-0017 的领域形状约定：
//! 单个类型化 DTO、不做通用 KV 透传。
//!
//! 全部命令 async 化（形状乙，spec #498 / #503）：目录创建/试写探针/指针
//! 文件读写与聚合时的指针读取是阻塞文件 IO，经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::commands::boot::current_boot;
use crate::db::data_location;
use crate::db::run_db;
use crate::error::{AppError, Result};

/// 默认应用数据目录（指针文件所在地，也是「恢复默认」的目标）。启动引导
/// 序列（commands::boot）与 DataLocation 信息聚合共用的同一解析点。
pub(crate) fn default_data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Io(format!("获取默认数据目录失败：{e}")))
}

/// 生效库目录（恢复通道共用，issue #601）：优先启动引导登记的生效目录，
/// 未登记时回退默认数据目录——恢复命令目标路径与启动失败重置共用的
/// 壳层解析点（领域解析见 [`data_location::effective_db_dir`]）。
pub(crate) fn effective_db_dir_of(app: &AppHandle) -> Result<PathBuf> {
    let default_dir = default_data_dir(app)?;
    let boot = current_boot(app);
    Ok(data_location::effective_db_dir(boot.as_ref(), &default_dir))
}

/// 获取 DataLocation 信息：当前生效目录、是否已更改待重启生效、回退警示。
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

/// 提交更改位置意图：三步校验通过后写入指针文件，下次启动搬迁生效。
#[tauri::command]
pub async fn submit_data_location_change(
    app: AppHandle,
    target_dir: String,
    adopt_existing: bool,
) -> Result<data_location::DataLocationChangeOutcome> {
    run_db("submit_data_location_change", move || {
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

/// 恢复默认位置：与更改完全相同的校验 + 写意图机制，目标是默认应用数据目录。
/// 默认目录可能仍保留搬迁前的旧库（原库永久保留），此时同样返回二选一信号。
#[tauri::command]
pub async fn restore_default_data_location(
    app: AppHandle,
    adopt_existing: bool,
) -> Result<data_location::DataLocationChangeOutcome> {
    run_db("restore_default_data_location", move || {
        let default_dir = default_data_dir(&app)?;
        data_location::validate_and_commit(&default_dir, &default_dir, adopt_existing)
    })
    .await
}
