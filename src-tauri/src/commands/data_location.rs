//! DataLocation 领域命令层（issue #133 / ADR-0018）。
//!
//! 在引导内核（[`crate::db::data_location`]）之上提供三个领域命令：
//! 查询当前 DataLocation 信息、校验并提交更改意图、恢复默认位置。
//! 保持 ADR-0017 的领域形状约定：单个类型化 DTO、不做通用 KV 透传。
//!
//! 「更改位置」三步校验（ADR-0018 决策 #2）：① 目录不存在则 `create_dir_all`
//! 自动创建；② 试写小临时文件验证可写后清理；③ 目标已有同名 `ledger.db` 时
//! 返回明确的「二选一」信号（接管该库 / 取消换位），由前端确认后二次提交
//! `adopt_existing = true`；不静默覆盖、也不解析既有库内容合法性。校验全部
//! 通过后才把意图写入指针文件，真实搬迁只发生在下次启动（引导内核完成）。

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::db::data_location;
use crate::error::{AppError, Result};

/// DataLocation 当前信息（issue #133）：设置页展示用。
#[derive(Debug, Serialize)]
pub struct DataLocationInfo {
    /// 当前生效的库文件目录（完整路径）。
    pub active_dir: String,
    /// 指针文件记录的意图目录；`None` = 未配置（缺失或损坏均视同未配置，
    /// 损坏时的警示由 `fallback_reason` 另行承载）。
    pub configured_dir: Option<String>,
    /// 已更改待重启生效：意图目录 ≠ 当前生效目录（意图已落盘、搬迁尚未发生）。
    pub pending_restart: bool,
    /// 上次启动引导发生回退的原因（供界面显著提示）；`None` = 未回退。
    pub fallback_reason: Option<String>,
}

/// 更改意图提交结果（issue #133）：更改位置与恢复默认共用。
#[derive(Debug, Serialize)]
pub struct DataLocationChangeOutcome {
    /// 目标已存在同名 `ledger.db`，需用户二选一（接管该库 / 取消换位）。
    /// 前端呈现确认后，以 `adopt_existing = true` 二次提交即接管落盘；
    /// 取消换位则不再提交，状态保持不变。
    pub requires_choice: bool,
    /// 意图是否已落盘（校验通过并写入指针文件，下次启动生效）。
    pub committed: bool,
    /// 已落盘意图的目标目录（`committed` 时有值）。
    pub target_dir: Option<String>,
}

/// 聚合 DataLocation 信息（引导结果可选）：命令层与 BDD 共用同一降级逻辑——
/// 引导结果未登记（异常时序）时按出厂行为降级：生效目录即默认目录。
pub fn gather_info_from_boot(
    default_dir: &Path,
    boot: Option<&data_location::Boot>,
) -> DataLocationInfo {
    let (active_dir, fallback) = match boot {
        Some(boot) => (boot.db_dir.clone(), boot.fallback_reason.clone()),
        None => (default_dir.to_path_buf(), None),
    };
    gather_info(default_dir, &active_dir, fallback.as_deref())
}

/// 聚合 DataLocation 信息：生效目录 / 意图目录 / 待重启生效 / 回退警示。
/// 命令层与 BDD 共用的内部实现；`active_dir` 与 `fallback` 来自启动期
/// 已登记的引导结果（[`data_location::Boot`]）。
pub fn gather_info(
    default_dir: &Path,
    active_dir: &Path,
    fallback_reason: Option<&str>,
) -> DataLocationInfo {
    let configured_dir = data_location::configured_intent(default_dir);
    let pending_restart = match &configured_dir {
        Some(intent) => intent != active_dir,
        None => false,
    };
    DataLocationInfo {
        active_dir: active_dir.to_string_lossy().into_owned(),
        configured_dir: configured_dir.map(|dir| dir.to_string_lossy().into_owned()),
        pending_restart,
        fallback_reason: fallback_reason.map(str::to_string),
    }
}

/// 可写性探针文件名（②试写的固定路径，BDD 用同名目录预占可稳定触发拒绝分支）。
pub const WRITE_PROBE_FILE_NAME: &str = ".ledger_write_probe";

/// 对目标目录执行三步校验，通过后把更改意图写入指针文件。
/// `adopt_existing`：目标已有同名 `ledger.db` 时是否接管（用户二选一后二次提交）。
/// 本命令不搬迁任何文件、不解析既有库内容；真实搬迁只发生在下次启动。
pub fn validate_and_commit(
    default_dir: &Path,
    target: &Path,
    adopt_existing: bool,
) -> Result<DataLocationChangeOutcome> {
    // ① 目录不存在则自动创建。
    std::fs::create_dir_all(target)
        .map_err(|e| AppError::Invalid(format!("无法创建目标目录（{}）：{e}", target.display())))?;

    // ② 试写小临时文件验证可写，用后即清。
    let probe = target.join(WRITE_PROBE_FILE_NAME);
    let probe_result = (|| -> std::io::Result<()> {
        std::fs::write(&probe, b"ok")?;
        std::fs::remove_file(&probe)
    })();
    probe_result
        .map_err(|e| AppError::Invalid(format!("目标目录不可写（{}）：{e}", target.display())))?;

    // ③ 目标已有同名库 → 返回二选一信号，不静默覆盖、不解析库内容。
    let target_db = target.join(data_location::DB_FILE_NAME);
    if target_db.exists() && !adopt_existing {
        tracing::info!(target = %target.display(), "目标位置已有同名库，返回二选一信号");
        return Ok(DataLocationChangeOutcome {
            requires_choice: true,
            committed: false,
            target_dir: None,
        });
    }

    // 校验通过 → 意图落盘（指针文件原子写入）。
    data_location::write_pointer(default_dir, target)?;
    tracing::info!(target = %target.display(), "DataLocation 更改意图已落盘，下次启动生效");
    Ok(DataLocationChangeOutcome {
        requires_choice: false,
        committed: true,
        target_dir: Some(target.to_string_lossy().into_owned()),
    })
}

/// 默认应用数据目录（指针文件所在地，也是「恢复默认」的目标）。
fn default_data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Io(format!("获取默认数据目录失败：{e}")))
}

/// 获取 DataLocation 信息：当前生效目录、是否已更改待重启生效、回退警示。
#[tauri::command]
pub fn get_data_location_info(app: AppHandle) -> Result<DataLocationInfo> {
    let default_dir = default_data_dir(&app)?;
    let boot = app
        .try_state::<data_location::Boot>()
        .map(|state| state.inner().clone());
    Ok(gather_info_from_boot(&default_dir, boot.as_ref()))
}

/// 提交更改位置意图：三步校验通过后写入指针文件，下次启动搬迁生效。
#[tauri::command]
pub fn submit_data_location_change(
    app: AppHandle,
    target_dir: String,
    adopt_existing: bool,
) -> Result<DataLocationChangeOutcome> {
    let default_dir = default_data_dir(&app)?;
    let trimmed = target_dir.trim();
    if trimmed.is_empty() {
        return Err(AppError::Invalid("目标目录不能为空".into()));
    }
    validate_and_commit(&default_dir, Path::new(trimmed), adopt_existing)
}

/// 恢复默认位置：与更改完全相同的校验 + 写意图机制，目标是默认应用数据目录。
/// 默认目录可能仍保留搬迁前的旧库（原库永久保留），此时同样返回二选一信号。
#[tauri::command]
pub fn restore_default_data_location(
    app: AppHandle,
    adopt_existing: bool,
) -> Result<DataLocationChangeOutcome> {
    let default_dir = default_data_dir(&app)?;
    validate_and_commit(&default_dir, &default_dir, adopt_existing)
}
