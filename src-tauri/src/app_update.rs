//! 应用更新——检查路径（issue #1429 / ADR-0124 决策 4/7/8）。
//!
//! 归属：壳层机制（ADR-0124 决策 7）——不进基础设施 crate（ADR-0111 归位判据：
//! 只被壳层消费）、不立业务域 crate（无账本语义）、不立壳内域目录；检查命令与
//! 启动延迟静默检查的同族接缝（下载/安装/更新前备份）归后端安装路径票 #1430，
//! 不在本模块。
//!
//! 接缝（全模块唯一 updater 调用点 = [`check_update`]，接缝数保持 1）：
//! - **手动检查**：`commands::app_update::check_for_updates` 薄壳 → 本模块
//!   [`check_update`]，返回版本比较三态（更新/相同/更旧）；失败就地报码化错误
//!   （`update.unreachable` / `update.manifest-invalid`，ADR-0050）。
//! - **启动延迟静默检查**：[`start_auto_update_check`] 经
//!   `start_background_services` 成组拉起（成对拉起守门
//!   `scripts/check-background-services.ts`），延迟触发一轮 [`run_silent_check`]；
//!   后台失败完全静默，发现新版本经 [`UPDATE_AVAILABLE_EVENT`] 通知前端
//!   （「已提醒版本」去重记忆归前端，本票不管）。
//! - **自动检查开关**：设备级偏好（ADR-0124 决策 8 / ADR-0017 修订记录先例）——
//!   真源在前端 localStorage（不随 Backup/Restore 迁移、不经账本数据分发），
//!   后端持运行期镜像 [`AutoUpdateSwitch`]（缺省按开启兜底），前端在启动与变更时
//!   经 `set_auto_update_enabled` 推送，静默检查据此决定是否发起。
//!
//! 平台：updater 插件桌面专属（上游仅桌面编译），移动端命令在位但报码化不支持
//! （更新入口按桌面专属管理动作对待，移动端为发布页引导，ADR-0124 决策 1）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use ledger_infra::error::AppError;
use ledger_infra::events::post_emit_with;

/// 更新可用前端通知事件（带 payload，不经失效信号映射 ADR-0044，先例同标的
/// 同步进度事件）：启动延迟静默检查发现新版本时发射，携版本号与发布说明
/// （兼容性警告随发布说明承载，ADR-0124 决策 5）；手动检查不发射——其结果
/// 由命令返回值就地呈现。
pub const UPDATE_AVAILABLE_EVENT: &str = "ledger:update-available";

/// 启动延迟（ADR-0124 决策 4「启动后延迟静默检查」）：业务可用起点拉起后再
/// 触发，避开启动期建库/解锁/参考数据加载的 IO 高峰。
const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(15);

/// 单次检查请求超时：手动检查与后台静默检查同源。体验取舍（仓库级）：等待成功
/// 优于即刻失败，10s 内不可达才报错/静默，不悬挂界面也不无限等。
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// 自动检查开关：运行期镜像（真源在前端 localStorage 设备偏好）
// ---------------------------------------------------------------------------

/// 自动检查开关的运行期镜像（应用级状态）：设备级开关，**新装默认开启**
/// （ADR-0124 决策 8）——未推送即按开启兜底，提醒侧另有「已提醒版本」记忆
/// 兜底不骚扰。刻意不入 `app_settings`：该表随 Backup/Restore 迁移，表达不了
/// 「这台检查、那台不检查」的设备级语义（ADR-0017 修订记录例外先例）。
///
/// 生命周期：进程级——`run()` setup 登记默认值，前端启动/变更时经
/// `set_auto_update_enabled` 推送更新；原位重引导不重置（设备偏好不随换库变化）。
#[derive(Debug)]
pub struct AutoUpdateSwitch(AtomicBool);

impl Default for AutoUpdateSwitch {
    fn default() -> Self {
        // 新装默认开启（ADR-0124 决策 8）。
        Self(AtomicBool::new(true))
    }
}

impl AutoUpdateSwitch {
    /// 推送开关到镜像（`set_auto_update_enabled` 命令的唯一写点）。
    pub fn set(&self, enabled: bool) {
        self.0.store(enabled, Ordering::SeqCst);
    }

    /// 读取镜像（启动延迟静默检查发起前判定）。
    fn is_enabled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// 检查结果形状（IPC 返回 + 事件载荷）
// ---------------------------------------------------------------------------

/// 版本比较三态（票面：返回版本比较三态——更新/相同/更旧）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateCheckStatus {
    /// 更新源版本高于当前版本。
    Update,
    /// 更新源版本与当前版本相同。
    Same,
    /// 更新源版本低于当前版本。
    Older,
}

/// 更新检查结果（IPC 返回形状）：发现新版本时携版本号与发布说明（兼容性警告
/// 随发布说明）；相同/更旧不携（前端据三态呈现「已是最新」类文案）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckOutcome {
    pub status: UpdateCheckStatus,
    pub version: Option<String>,
    pub notes: Option<String>,
}

/// [`UPDATE_AVAILABLE_EVENT`] 事件载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAvailablePayload {
    pub version: String,
    pub notes: Option<String>,
}

// ---------------------------------------------------------------------------
// 码化错误（ADR-0050：用户可见错误经码化构造器 + errors.json 模板）
// ---------------------------------------------------------------------------

/// 清单畸形码化错误构造单点。
#[cfg(desktop)]
fn manifest_invalid_error() -> AppError {
    AppError::coded(
        "update.manifest-invalid",
        "更新服务返回了无法解析的更新清单",
    )
}

/// 更新清单异常 → 码化错误映射单点（票面两条异常路径：不可达、畸形清单）：
/// - 清单解析 / 平台条目缺失 / 版本号非法 → `update.manifest-invalid`；
/// - 传输级失败、端点全部未取得清单（HTTP 非 2xx）、配置缺端点（生产配置烧入
///   端点数组，此形态等同源不可用）→ `update.unreachable`。
///
/// 插件错误类型 `#[non_exhaustive]`，未知变体按不可达兜底（宁报「暂不可用」
/// 不误报「清单损坏」）。
#[cfg(desktop)]
fn map_check_error(error: tauri_plugin_updater::Error) -> AppError {
    use tauri_plugin_updater::Error as UpdaterError;
    let message = error.to_string();
    tracing::warn!(error = %message, "更新检查失败");
    match &error {
        UpdaterError::Serialization(_)
        | UpdaterError::Semver(_)
        | UpdaterError::TargetNotFound(_)
        | UpdaterError::TargetsNotFound(_) => manifest_invalid_error(),
        _ => AppError::coded("update.unreachable", "更新源暂时不可用，请检查网络后重试"),
    }
}

/// 移动端命令面的码化不支持错误（ADR-0124 决策 1：deb 与 Android 无更新工件，
/// 更新入口降级为发布页引导，不冒充可自更新）。
pub fn unsupported_platform_error() -> AppError {
    AppError::coded(
        "update.platform-unsupported",
        "当前平台不支持应用内更新，请前往发布页下载新版本",
    )
}

// ---------------------------------------------------------------------------
// 更新检查（唯一 updater 调用点）
// ---------------------------------------------------------------------------

/// 更新检查唯一调用点（ADR-0124 测试决策：插件调用点保持薄，接缝数保持 1）。
/// 端点接 [`endpoint_override`]（测试注入指向本地真 HTTP 更新源；生产传 `None`
/// 走应用配置的更新源端点数组——按序尝试，镜像扩展位只改配置不改代码）。
///
/// 三态判定收本单点：版本比较器放行任意远端版本（插件默认比较器只放行更新，
/// 相同/更旧无从区分），分类在本函数完成——远端 semver 高于当前为
/// [`UpdateCheckStatus::Update`]，相等为 `Same`，更低为 `Older`；清单应答
/// `204 No Content`（无内容）视同 `Same`。
#[cfg(desktop)]
pub async fn check_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    endpoint_override: Option<Vec<url::Url>>,
) -> ledger_infra::error::Result<UpdateCheckOutcome> {
    use tauri_plugin_updater::UpdaterExt;

    let mut builder = app
        .updater_builder()
        .version_comparator(|_, _| true)
        .timeout(CHECK_TIMEOUT);
    if let Some(endpoints) = endpoint_override {
        builder = builder.endpoints(endpoints).map_err(map_check_error)?;
    }
    let updater = builder.build().map_err(map_check_error)?;
    match updater.check().await {
        Ok(Some(update)) => {
            let current = semver::Version::parse(&update.current_version)
                .map_err(|_| manifest_invalid_error())?;
            let remote =
                semver::Version::parse(&update.version).map_err(|_| manifest_invalid_error())?;
            let status = match remote.cmp(&current) {
                std::cmp::Ordering::Greater => UpdateCheckStatus::Update,
                std::cmp::Ordering::Equal => UpdateCheckStatus::Same,
                std::cmp::Ordering::Less => UpdateCheckStatus::Older,
            };
            // 仅发现新版本时返回版本号与发布说明；相同/更旧不携。
            if status == UpdateCheckStatus::Update {
                Ok(UpdateCheckOutcome {
                    status,
                    version: Some(update.version),
                    notes: update.body,
                })
            } else {
                Ok(UpdateCheckOutcome {
                    status,
                    version: None,
                    notes: None,
                })
            }
        }
        // 204 No Content：更新源无清单内容，视同「相同」。
        Ok(None) => Ok(UpdateCheckOutcome {
            status: UpdateCheckStatus::Same,
            version: None,
            notes: None,
        }),
        Err(error) => Err(map_check_error(error)),
    }
}

// ---------------------------------------------------------------------------
// 启动延迟静默检查（后台服务）
// ---------------------------------------------------------------------------

/// 启动延迟静默检查的单轮执行体（延迟触发后跑一次）：开关镜像关闭时空转不发
/// 请求；发现新版本经 [`UPDATE_AVAILABLE_EVENT`] 通知前端（主线程非阻塞投递，
/// spec #364）；任何失败完全静默（仅日志，ADR-0124 决策 4）。端点注入接缝同
/// [`check_update`]。返回检查结果供测试观察（开关关闭或检查失败为 `None`）；
/// 生产定时任务忽略返回值。
#[cfg(desktop)]
pub async fn run_silent_check<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    endpoint_override: Option<Vec<url::Url>>,
) -> Option<UpdateCheckOutcome> {
    use tauri::Manager;
    if !app.state::<AutoUpdateSwitch>().is_enabled() {
        return None;
    }
    match check_update(app, endpoint_override).await {
        Ok(outcome) => {
            if outcome.status == UpdateCheckStatus::Update {
                emit_update_available(app, &outcome);
            }
            Some(outcome)
        }
        Err(error) => {
            tracing::info!(?error, "启动延迟静默检查失败，静默忽略不打扰用户");
            None
        }
    }
}

/// 发射更新可用事件（版本号在 Update 态恒有，分类单点保证；缺版本防御性跳过）。
#[cfg(desktop)]
fn emit_update_available<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    outcome: &UpdateCheckOutcome,
) {
    use tauri::Emitter;
    let Some(version) = outcome.version.clone() else {
        return;
    };
    let payload = UpdateAvailablePayload {
        version,
        notes: outcome.notes.clone(),
    };
    let handle = app.clone();
    post_emit_with(app, move || {
        let _ = handle.emit(UPDATE_AVAILABLE_EVENT, payload);
    });
}

/// 单次拉起守卫：原位重引导重复调用幂等（ADR-0080，先例同各域调度）。
#[cfg(desktop)]
static AUTO_CHECK_SPAWNED: AtomicBool = AtomicBool::new(false);

/// 启动延迟静默检查后台服务（ADR-0124 决策 4）：业务可用起点经壳层唯一编排点
/// `start_background_services` 成组拉起（成对拉起守门
/// `scripts/check-background-services.ts`，删除接线即红）；延迟
/// [`STARTUP_CHECK_DELAY`] 后跑一轮 [`run_silent_check`]，其余语义见其文档。
pub fn start_auto_update_check<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(desktop)]
    {
        if AUTO_CHECK_SPAWNED.swap(true, Ordering::SeqCst) {
            return;
        }
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(STARTUP_CHECK_DELAY).await;
            run_silent_check(&handle, None).await;
        });
    }
    #[cfg(mobile)]
    {
        // 移动端无 updater 通道（ADR-0124 决策 1）：本服务在移动端为无操作，
        // 编排点仍统一调用（成组拉起守门不分平台）。
        let _ = app;
    }
}
