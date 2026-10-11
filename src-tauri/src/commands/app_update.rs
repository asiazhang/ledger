//! 应用更新命令面（issue #1429 / ADR-0124 决策 7）：检查路径 IPC 命令壳。
//! 机制本体住 `crate::app_update`（壳层机制：不进基础设施、不立业务域 crate
//! 与壳内域目录）；本模块只做命令形状、平台门与开关推送。

use tauri::AppHandle;
use tauri::Manager;

use ledger_infra::error::Result;

use crate::app_update::{self, AutoUpdateSwitch, UpdateCheckOutcome};

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
