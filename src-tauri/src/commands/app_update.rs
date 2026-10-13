//! 应用更新命令面（issue #1429 / #1430 / ADR-0124 决策 7）：检查路径与安装路径
//! IPC 命令壳。机制本体住 `crate::app_update`（壳层机制：不进基础设施、不立
//! 业务域 crate 与壳内域目录）；本模块只做命令形状、平台门与开关推送。

use tauri::AppHandle;
use tauri::Manager;

use ledger_infra::error::Result;

use crate::app_update::{self, AutoUpdateSwitch, UpdateCheckOutcome, UpdateDownloadOutcome};

/// 手动检查更新（设置页，issue #1429 / ADR-0124 决策 4）：返回版本比较三态
/// （更新/相同/更旧），发现新版本携版本号与发布说明（兼容性警告随发布说明）；
/// 失败就地报码化错误（`update.unreachable` / `update.manifest-invalid`）。
/// 设置页手动检查不受「已提醒版本」记忆限制（记忆归前端）。
#[tauri::command]
pub async fn check_for_updates<R: tauri::Runtime>(app: AppHandle<R>) -> Result<UpdateCheckOutcome> {
    #[cfg(desktop)]
    {
        app_update::check_update(&app, None).await
    }
    #[cfg(mobile)]
    {
        // 移动端无更新通道（ADR-0124 决策 1）：命令在位但报码化不支持，
        // 前端更新入口在移动端呈现发布页引导形态。
        let _ = app;
        Err(app_update::unsupported_platform_error())
    }
}

/// 下载更新（安装路径第 1 步，issue #1430 / ADR-0124 决策 6）：用户确认下载后
/// 调用——先为当前活动账本创建更新前备份（失败报 `update.backup-failed` 并
/// 中止，不带病升级），后带进度下载更新工件（进度经
/// `ledger:update-download-progress` 事件对前端可观察），完成后等待二次确认
/// 安装。全询问式：本命令即下载确认入口，无任何静默下载路径。
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
        // 移动端无更新通道（ADR-0124 决策 1）：命令在位但报码化不支持。
        let _ = app;
        Err(app_update::unsupported_platform_error())
    }
}

/// 安装更新（安装路径第 2 步，issue #1430 / ADR-0124 决策 6）：下载完成后的
/// 二次确认入口——安装已下载工件并自动重启进入新版；未下载报
/// `update.not-downloaded`。永不静默安装：本命令即安装确认入口。成功路径
/// 不返回（进程随安装重启），失败就地报码化错误。
#[tauri::command]
pub fn install_update<R: tauri::Runtime>(app: AppHandle<R>) -> Result<()> {
    #[cfg(desktop)]
    {
        app_update::install_pending_update(&app)
    }
    #[cfg(mobile)]
    {
        // 移动端无更新通道（ADR-0124 决策 1）：命令在位但报码化不支持。
        let _ = app;
        Err(app_update::unsupported_platform_error())
    }
}

/// 推送设备级「自动检查更新」开关到后端运行期镜像（issue #1429 / ADR-0124 决策 8）：
/// 真源在前端 localStorage 设备偏好（不随 Backup/Restore 迁移、新装默认开启），
/// 前端在应用启动与开关变更时经本命令推送（`set_auto_execution_enabled` 同形
/// 先例，ADR-0017 修订记录）；启动延迟静默检查据此决定是否发起检查请求。
#[tauri::command]
pub fn set_auto_update_enabled<R: tauri::Runtime>(app: AppHandle<R>, enabled: bool) -> Result<()> {
    app.state::<AutoUpdateSwitch>().set(enabled);
    tracing::info!(enabled, "自动检查更新开关镜像已更新");
    Ok(())
}
