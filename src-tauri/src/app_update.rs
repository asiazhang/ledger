//! 应用更新——检查路径与安装路径（issue #1429 / #1430，ADR-0124 决策 4/6/7/8）。
//!
//! 归属：壳层机制（ADR-0124 决策 7）——不进基础设施 crate（ADR-0111 归位判据：
//! 只被壳层消费）、不立业务域 crate（无账本语义）、不立壳内域目录。
//!
//! 接缝（全模块唯一 updater 调用点 = [`fetch_remote_update`]，接缝数保持 1）：
//! - **手动检查**：`commands::app_update::check_for_updates` 薄壳 → 本模块
//!   [`check_update`]，返回版本比较三态（更新/相同/更旧）；失败就地报码化错误
//!   （`update.unreachable` / `update.manifest-invalid`，ADR-0050）。
//! - **安装路径（issue #1430）**：全询问式编排——`download_update`（用户确认
//! - **安装路径（issue #1430 / #1944）**：全询问式编排——`download_update`
//!   （用户确认下载后调用）经唯一调用点取更新对象并带进度下载（进度经
//!   [`UPDATE_DOWNLOAD_PROGRESS_EVENT`] 对前端可观察，主线程非阻塞投递），
//!   下载完成的工件暂存 [`DownloadedUpdate`]；`install_pending_update`
//!   （二次确认后调用）**先为当前活动账本创建更新前备份**（备份域
//!   `create_pre_update_backup`，失败即中止安装并报 `update.backup-failed`
//!   ——不带病升级，已下载工件留在暂存可重试，ADR-0124 决策 6 / #1944），
//!   后安装并自动重启进入新版。永不静默下载、永不静默安装。
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
    AppError::coded("update.manifest-invalid", "更新源返回了无法解析的更新清单")
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
// 更新入口形态（issue #1431 平台分支信号，ADR-0124 决策 1）
// ---------------------------------------------------------------------------

/// 更新入口形态闭集：`updater` = 桌面三平台完整更新面（检查/下载/安装）；
/// `release-page` = deb 与 Android——无更新器可消费的更新工件，更新入口降级为
/// 「打开发布页」引导，不冒充可自更新（ADR-0124 决策 1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateSurface {
    /// 完整更新面：检查、带进度下载、确认安装重启。
    Updater,
    /// 发布页引导：应用内无下载/安装动作。
    ReleasePage,
}

/// 平台 → 更新入口形态判定（纯函数，`APPIMAGE` 环境变量观测以参数注入，单测
/// 无需触碰进程环境）：
/// - 移动端（Android）：release-page；
/// - Linux 桌面：AppImage 运行时才持更新工件——AppImage runtime 启动时注入
///   `APPIMAGE` 环境变量，是 AppImage 与 deb 两种 Linux 安装形态的唯一运行期判据；
///   deb 及其他 Linux 形态 → release-page；
/// - macOS / Windows：updater。
pub fn update_surface(appimage_env: Option<&std::ffi::OsStr>) -> UpdateSurface {
    // 移动端（Android）恒发布页引导；Linux 桌面仅 AppImage 运行时持更新工件
    // （`APPIMAGE` 由 AppImage runtime 注入，deb 等其他形态未注入）；其余桌面
    // updater。`cfg!` 为编译期常量分支，全平台可编译，无死代码。
    if cfg!(mobile) {
        return UpdateSurface::ReleasePage;
    }
    if cfg!(target_os = "linux") && appimage_env.is_none() {
        return UpdateSurface::ReleasePage;
    }
    UpdateSurface::Updater
}

// ---------------------------------------------------------------------------
// 更新检查（唯一 updater 调用点）
// ---------------------------------------------------------------------------

/// 更新清单获取唯一调用点（ADR-0124 测试决策：插件调用点保持薄，接缝数保持 1）。
/// 端点接 [`endpoint_override`]（测试注入指向本地真 HTTP 更新源；生产传 `None`
/// 走应用配置的更新源端点数组——按序尝试，镜像扩展位只改配置不改代码）。
///
/// 返回 `None` = 更新源无清单内容（`204 No Content`）或远端版本不高
/// （插件默认比较器语义）；`Some(update)` = 插件交出的更新对象——版本分类
/// （更新/相同/更旧三态）与消费（检查结果 / 下载安装）由调用方完成。
#[cfg(desktop)]
async fn fetch_remote_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    endpoint_override: Option<Vec<url::Url>>,
) -> ledger_infra::error::Result<Option<tauri_plugin_updater::Update>> {
    use tauri_plugin_updater::UpdaterExt;

    let mut builder = app
        .updater_builder()
        .version_comparator(|_, _| true)
        .timeout(CHECK_TIMEOUT);
    if let Some(endpoints) = endpoint_override {
        builder = builder.endpoints(endpoints).map_err(map_check_error)?;
    }
    let updater = builder.build().map_err(map_check_error)?;
    updater.check().await.map_err(map_check_error)
}

/// 版本三态分类（与 [`check_update`] / [`download_update`] 共用的单点）：
/// 远端 semver 高于当前为 [`UpdateCheckStatus::Update`]，相等为 `Same`，
/// 更低为 `Older`。清单版本号非法报 `update.manifest-invalid`。
#[cfg(desktop)]
fn version_status(
    update: &tauri_plugin_updater::Update,
) -> ledger_infra::error::Result<UpdateCheckStatus> {
    let current =
        semver::Version::parse(&update.current_version).map_err(|_| manifest_invalid_error())?;
    let remote = semver::Version::parse(&update.version).map_err(|_| manifest_invalid_error())?;
    Ok(match remote.cmp(&current) {
        std::cmp::Ordering::Greater => UpdateCheckStatus::Update,
        std::cmp::Ordering::Equal => UpdateCheckStatus::Same,
        std::cmp::Ordering::Less => UpdateCheckStatus::Older,
    })
}

/// 更新检查（检查路径命令的机制本体，issue #1429）：三态判定收本单点——
/// 远端 semver 高于当前为 [`UpdateCheckStatus::Update`]，相等为 `Same`，
/// 更低为 `Older`。
#[cfg(desktop)]
pub async fn check_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    endpoint_override: Option<Vec<url::Url>>,
) -> ledger_infra::error::Result<UpdateCheckOutcome> {
    let Some(update) = fetch_remote_update(app, endpoint_override).await? else {
        return Ok(UpdateCheckOutcome {
            status: UpdateCheckStatus::Same,
            version: None,
            notes: None,
        });
    };
    let status = version_status(&update)?;
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

// ---------------------------------------------------------------------------
// 安装路径（issue #1430 / ADR-0124 决策 6）：更新前备份 → 带进度下载 → 安装重启
// ---------------------------------------------------------------------------

/// 下载进度事件（带 payload，不经失效信号映射 ADR-0044，先例同
/// [`UPDATE_AVAILABLE_EVENT`] 与标的同步进度事件）：`download_update` 编排
/// 下载期间发射，携累计接收字节数与总大小（清单未给出总大小时为空）。
pub const UPDATE_DOWNLOAD_PROGRESS_EVENT: &str = "ledger:update-download-progress";

/// [`UPDATE_DOWNLOAD_PROGRESS_EVENT`] 事件载荷（测试侧经事件反序列化观察，
/// 与 [`UpdateAvailablePayload`] 同形）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadProgress {
    /// 累计已接收字节数。
    pub received: u64,
    /// 工件总大小（服务端未给出 Content-Length 时为空）。
    pub total: Option<u64>,
}

/// 下载完成结果（IPC 返回形状）：版本号供前端呈现「已下载 x.y.z，等待安装确认」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadOutcome {
    pub version: String,
}

/// 进度事件发射节流间隔：本地高带宽下 chunk 回调可达每秒数百次，逐 chunk
/// 发射会淹没 IPC；时间节流（加收尾必发一次）保证前端可观察又不刷屏。
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);

/// 下载完成工件的暂存槽（应用级状态，桌面专属——updater 插件桌面编译）：
/// `download_update` 成功后存入（更新对象 + 已验签工件字节），
/// `install_pending_update` 消费（安装成功取走即清；安装失败放回暂存，现场
/// 可重试）。二次确认前的重复下载覆盖暂存；重启后自然清零（进程内状态，
/// 工件不落盘——更新器各平台安装器自管临时文件）。
#[cfg(desktop)]
#[derive(Default)]
pub struct DownloadedUpdate(std::sync::Mutex<Option<StagedUpdate>>);

/// 暂存载荷：验签通过的更新对象 + 工件字节，等待二次确认安装。
#[cfg(desktop)]
struct StagedUpdate {
    update: tauri_plugin_updater::Update,
    bytes: Vec<u8>,
}

#[cfg(desktop)]
impl DownloadedUpdate {
    fn store(&self, pending: StagedUpdate) {
        *self.lock() = Some(pending);
    }

    fn take(&self) -> Option<StagedUpdate> {
        self.lock().take()
    }

    /// 是否有已下载待安装的工件（前端状态与测试观察用）。
    pub fn is_pending(&self) -> bool {
        self.lock().is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<StagedUpdate>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 更新前备份目录换装槽（测试注入桩优先，生产缺省 `None` → 应用数据目录；
/// 先例同同步域 `SyncChannelsSlot` 管理态换装）：仅测试注入临时目录，避免
/// 集成测试写真实用户数据目录、并让「备份失败」可确定性注入（指向坏路径）。
#[derive(Default)]
pub struct PreUpdateBackupDirSlot(pub Option<std::path::PathBuf>);

/// 更新前备份失败的码化错误（ADR-0050）：中止安装——不带病升级（ADR-0124
/// 决策 6），用户明确得知备份未成、安装未继续；已下载工件留在暂存可重试。
fn backup_failed_error() -> AppError {
    AppError::coded(
        "update.backup-failed",
        "更新前备份创建失败，已中止安装；请排查磁盘与权限后重试",
    )
}

/// 无可下载更新的码化错误：下载确认到达时更新源已无更高版本（提醒与确认
/// 之间被其他设备/手动操作抢先，或清单回退）。
fn no_update_error() -> AppError {
    AppError::coded("update.no-update", "当前没有可安装的更新")
}

/// 下载失败的码化错误（传输层）：工件未到手或未落暂存。
fn download_failed_error() -> AppError {
    AppError::coded("update.download-failed", "更新下载失败，请检查网络后重试")
}

/// 验签失败的码化错误：更新工件未通过更新签名验证——拒绝安装（安全边界，
/// 单列错误码以便反馈排查）。
fn signature_invalid_error() -> AppError {
    AppError::coded("update.signature-invalid", "更新工件验签失败，已拒绝安装")
}

/// 未下载即安装的码化错误：安装确认到达但暂存槽为空（下载未完成或已消费）。
fn not_downloaded_error() -> AppError {
    AppError::coded("update.not-downloaded", "更新尚未下载完成，请先下载")
}

/// 安装失败的码化错误（安装器执行层）。
fn install_failed_error() -> AppError {
    AppError::coded("update.install-failed", "更新安装失败，请重试")
}

/// 下载阶段错误 → 码化错误映射单点：验签失败单列（安全边界：签名解码 /
/// minisign 验证失败都指向「工件不可信」），其余传输/IO 形态归下载失败。
/// 插件错误类型 `#[non_exhaustive]`，未知变体按下载失败兜底。
#[cfg(desktop)]
fn map_download_error(error: tauri_plugin_updater::Error) -> AppError {
    use tauri_plugin_updater::Error as UpdaterError;
    let message = error.to_string();
    tracing::warn!(error = %message, "更新下载失败");
    match &error {
        UpdaterError::Minisign(_) | UpdaterError::SignatureUtf8(_) | UpdaterError::Base64(_) => {
            signature_invalid_error()
        }
        _ => download_failed_error(),
    }
}

/// 更新前备份目录解析单点：换装槽优先（测试注入），缺省落应用数据目录
/// （与 RestoreSafetyBackup 同族落点——独立于用户备份目录，词汇表
/// PreUpdateBackup 词条）。
fn pre_update_backup_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> ledger_infra::error::Result<std::path::PathBuf> {
    use tauri::Manager;
    if let Some(slot) = app.try_state::<PreUpdateBackupDirSlot>()
        && let Some(dir) = &slot.0
    {
        return Ok(dir.clone());
    }
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Io(format!("获取应用数据目录失败：{e}")))
}

/// 为当前活动账本创建更新前备份（**安装编排第 1 步**，安装确认后、旧版退出前
/// 执行，#1944 / ADR-0124 决策 6）：备份域 `create_pre_update_backup` 经写槽
/// **裸作业**投递（不经统一写入口——不置脏、调度状态零接触，先例同
/// `create_backup` 命令）；失败原样映射 [`backup_failed_error`] 由编排中止安装
/// ——已下载工件留在暂存，重试无需重新下载。作业名注册了备份类持锁预算
/// （`VACUUM INTO` 全库拷贝是合法重作业）。
#[cfg(desktop)]
async fn create_pre_update_backup<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> ledger_infra::error::Result<()> {
    use tauri::Manager;
    let dir = pre_update_backup_dir(app)?;
    let version = app.package_info().version.to_string();
    let db = app.state::<ledger_infra::db::DbState>().write_handle();
    let command = "update.pre-update-backup";
    db.run_raw(command, move |conn| {
        ledger_backup::create_pre_update_backup(conn, &dir, &version).map(|_| ())
    })
    .await
    .map_err(|error| {
        tracing::error!(%error, "更新前备份创建失败，中止更新");
        backup_failed_error()
    })
}

/// 单次进度发射（主线程非阻塞投递，spec #364，先例同更新可用事件）。
#[cfg(desktop)]
fn emit_download_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    received: u64,
    total: Option<u64>,
) {
    let payload = UpdateDownloadProgress { received, total };
    let handle = app.clone();
    post_emit_with(app, move || {
        use tauri::Emitter;
        let _ = handle.emit(UPDATE_DOWNLOAD_PROGRESS_EVENT, payload);
    });
}

/// 带进度下载更新工件（下载编排收尾步）：进度经 [`UPDATE_DOWNLOAD_PROGRESS_EVENT`]
/// 发射（时间节流 + 收尾必发）；字节在插件内完成更新签名验证后返回，验签失败
/// 映射 [`signature_invalid_error`]。
#[cfg(desktop)]
async fn download_with_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    update: &tauri_plugin_updater::Update,
) -> ledger_infra::error::Result<Vec<u8>> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    // 进度累计跨两个闭包共享：闭包必须 Send（async 命令的 Future 约束），
    // 用原子量与互斥体而非 Cell。锁无 panic 路径不会中毒，中毒分支防御性
    // 进内取值（ADR-0060 同族）。
    let received = AtomicU64::new(0);
    let last_emit = Mutex::new(std::time::Instant::now() - PROGRESS_EMIT_INTERVAL);
    // 服务端声明的总大小（chunk 回调携带；收尾回调节无参，由最后一片转交）。
    let last_total = Mutex::new(None);
    let bytes = update
        .download(
            |chunk, total| {
                let received_now =
                    received.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
                if total.is_some() {
                    *last_total.lock().unwrap_or_else(|e| e.into_inner()) = total;
                }
                // 时间节流：窗口内的 chunk 静默累计，窗口到期才发射（判定块内
                // 换值后即放锁，发射在锁外）。
                let due = {
                    let mut last = last_emit.lock().unwrap_or_else(|e| e.into_inner());
                    if last.elapsed() >= PROGRESS_EMIT_INTERVAL {
                        *last = std::time::Instant::now();
                        true
                    } else {
                        false
                    }
                };
                if due {
                    emit_download_progress(app, received_now, total);
                }
            },
            // 收尾必发：最终进度（received == total）不依赖节流窗口，总大小随
            // 最后一片转交（前端终态百分比可算）。
            || {
                emit_download_progress(
                    app,
                    received.load(Ordering::Relaxed),
                    *last_total.lock().unwrap_or_else(|e| e.into_inner()),
                );
            },
        )
        .await
        .map_err(map_download_error)?;
    tracing::info!(version = %update.version, bytes = bytes.len(), "更新工件下载完成");
    Ok(bytes)
}

/// 下载更新（安装路径第 1 步，issue #1430 / #1944）：经唯一调用点取更新对象
/// （无更高版本报 `update.no-update`），带进度下载并暂存到
/// [`DownloadedUpdate`]。调用时机 = 用户确认下载之后（全询问式：永不静默下载）；
/// 安装另有二次确认（[`install_pending_update`]）。下载编排零备份动作、
/// 零账本接触——更新前备份移至安装确认后（ADR-0124 决策 6，#1944 裁决落地）。
#[cfg(desktop)]
pub async fn download_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    endpoint_override: Option<Vec<url::Url>>,
) -> ledger_infra::error::Result<UpdateDownloadOutcome> {
    use tauri::Manager;
    // 1. 取更新对象：仅更高版本可装（检查路径三态语义同一单点）。
    let update = match fetch_remote_update(app, endpoint_override).await? {
        Some(update) if version_status(&update)? == UpdateCheckStatus::Update => update,
        _ => return Err(no_update_error()),
    };

    // 2. 带进度下载（插件内验签）并暂存，等待二次确认安装。暂存槽由壳层
    //    setup 恒登记（缺槽 = 接线缺陷，state() fail-loud，与
    //    `set_auto_update_enabled` 读开关镜像同形）。
    let bytes = download_with_progress(app, &update).await?;
    app.state::<DownloadedUpdate>().store(StagedUpdate {
        update: update.clone(),
        bytes,
    });
    Ok(UpdateDownloadOutcome {
        version: update.version,
    })
}

/// 安装已下载的更新（二次确认后的编排终点，issue #1430 / #1944）：**先更新前
/// 备份、后安装**——先为当前活动账本创建更新前备份（失败即中止安装并报
/// [`backup_failed_error`]，已下载工件留在暂存、重试无需重新下载——不带病升级，
/// ADR-0124 决策 6）；后从 [`DownloadedUpdate`] 取走工件（无暂存报
/// [`not_downloaded_error`]）交给插件安装。平台差异由插件承担（ADR-0124 决策 7）
/// ——Windows 启动 NSIS 安装器后进程退出（安装器默认拉起新版）；macOS / Linux
/// 替换完成后由本函数原地重启进程进入新版（真实重启链路归手动冒烟）。永不静默
/// 安装：本函数只有二次确认这一个调用入口。
#[cfg(desktop)]
pub async fn install_pending_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> ledger_infra::error::Result<()> {
    use tauri::Manager;

    // 1. 更新前备份：安装确认后、旧版退出前（ADR-0124 决策 6 / #1944）。失败即
    //    中止安装——编排次序可观察（集成测试断言备份失败时安装中止且暂存工件
    //    保留、重试零重新下载）。
    create_pre_update_backup(app).await?;

    // 2. 取走已暂存工件交给插件安装（无暂存报 `update.not-downloaded`）。
    let slot = app.state::<DownloadedUpdate>();
    let Some(StagedUpdate { update, bytes }) = slot.take() else {
        return Err(not_downloaded_error());
    };
    let version = update.version.clone();
    if let Err(error) = update.install(&bytes) {
        // 工件放回暂存：安装失败现场可重试，不必整轮重下（字节未消费；等待
        // 成功优于即刻失败的体验取舍）。Windows 安装器失败即进程退出到不了
        // 这里，放回只服务 macOS / Linux 的可恢复失败。
        slot.store(StagedUpdate { update, bytes });
        let message = error.to_string();
        tracing::warn!(error = %message, "更新安装失败，工件已放回暂存可重试");
        return Err(install_failed_error());
    }
    // Windows 上 install() 在启动安装器后直接退出进程，到不了这里；
    // macOS / Linux 替换完成，重启进程进入新版（diverging：不返回）。
    tracing::info!(version = %version, "更新安装完成，重启应用");
    app.restart()
}

#[cfg(test)]
mod surface_tests {
    use super::*;

    /// Linux 桌面：AppImage 运行时（`APPIMAGE` 注入）→ updater；deb 形态（未注入）
    /// → release-page（ADR-0124 决策 1：deb 无更新工件）。
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_surface_branches_on_appimage_env() {
        assert_eq!(
            update_surface(Some(std::ffi::OsStr::new("/opt/App.OpenLedger.AppImage"))),
            UpdateSurface::Updater
        );
        assert_eq!(update_surface(None), UpdateSurface::ReleasePage);
    }

    /// macOS / Windows：完整更新面（环境变量观测不参与判定）。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn desktop_non_linux_surface_is_updater() {
        assert_eq!(update_surface(None), UpdateSurface::Updater);
        assert_eq!(
            update_surface(Some(std::ffi::OsStr::new("anything"))),
            UpdateSurface::Updater
        );
    }

    /// 移动端：恒为发布页引导。
    #[cfg(mobile)]
    #[test]
    fn mobile_surface_is_release_page() {
        assert_eq!(update_surface(None), UpdateSurface::ReleasePage);
    }
}
