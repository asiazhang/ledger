use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use ledger_infra::error::AppError;
use ledger_infra::events::post_emit_with;

pub const UPDATE_AVAILABLE_EVENT: &str = "ledger:update-available";

const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(15);

const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct AutoUpdateSwitch(AtomicBool);

impl Default for AutoUpdateSwitch {
    fn default() -> Self {
        Self(AtomicBool::new(true))
    }
}

impl AutoUpdateSwitch {
    pub fn set(&self, enabled: bool) {
        self.0.store(enabled, Ordering::SeqCst);
    }

    fn is_enabled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateCheckStatus {
    Update,
    Same,
    Older,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckOutcome {
    pub status: UpdateCheckStatus,
    pub version: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAvailablePayload {
    pub version: String,
    pub notes: Option<String>,
}

#[cfg(desktop)]
fn manifest_invalid_error() -> AppError {
    AppError::coded("update.manifest-invalid", "更新源返回了无法解析的更新清单")
}

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

pub fn unsupported_platform_error() -> AppError {
    AppError::coded(
        "update.platform-unsupported",
        "当前平台不支持应用内更新，请前往发布页下载新版本",
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateSurface {
    Updater,
    ReleasePage,
}

pub fn update_surface(appimage_env: Option<&std::ffi::OsStr>) -> UpdateSurface {
    if cfg!(mobile) {
        return UpdateSurface::ReleasePage;
    }
    if cfg!(target_os = "linux") && appimage_env.is_none() {
        return UpdateSurface::ReleasePage;
    }
    UpdateSurface::Updater
}

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

#[cfg(desktop)]
static AUTO_CHECK_SPAWNED: AtomicBool = AtomicBool::new(false);

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
        let _ = app;
    }
}

pub const UPDATE_DOWNLOAD_PROGRESS_EVENT: &str = "ledger:update-download-progress";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadProgress {
    pub received: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadOutcome {
    pub version: String,
}

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(desktop)]
#[derive(Default)]
pub struct DownloadedUpdate(std::sync::Mutex<Option<StagedUpdate>>);

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

    pub fn is_pending(&self) -> bool {
        self.lock().is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<StagedUpdate>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Default)]
pub struct PreUpdateBackupDirSlot(pub Option<std::path::PathBuf>);

fn backup_failed_error() -> AppError {
    AppError::coded(
        "update.backup-failed",
        "更新前备份创建失败，已中止安装；请排查磁盘与权限后重试",
    )
}

fn no_update_error() -> AppError {
    AppError::coded("update.no-update", "当前没有可安装的更新")
}

fn download_failed_error() -> AppError {
    AppError::coded("update.download-failed", "更新下载失败，请检查网络后重试")
}

fn signature_invalid_error() -> AppError {
    AppError::coded("update.signature-invalid", "更新工件验签失败，已拒绝安装")
}

fn not_downloaded_error() -> AppError {
    AppError::coded("update.not-downloaded", "更新尚未下载完成，请先下载")
}

fn install_failed_error() -> AppError {
    AppError::coded("update.install-failed", "更新安装失败，请重试")
}

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

#[cfg(desktop)]
async fn download_with_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    update: &tauri_plugin_updater::Update,
) -> ledger_infra::error::Result<Vec<u8>> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    let received = AtomicU64::new(0);
    let last_emit = Mutex::new(std::time::Instant::now() - PROGRESS_EMIT_INTERVAL);
    let last_total = Mutex::new(None);
    let bytes = update
        .download(
            |chunk, total| {
                let received_now =
                    received.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
                if total.is_some() {
                    *last_total.lock().unwrap_or_else(|e| e.into_inner()) = total;
                }
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

#[cfg(desktop)]
pub async fn download_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    endpoint_override: Option<Vec<url::Url>>,
) -> ledger_infra::error::Result<UpdateDownloadOutcome> {
    use tauri::Manager;
    let update = match fetch_remote_update(app, endpoint_override).await? {
        Some(update) if version_status(&update)? == UpdateCheckStatus::Update => update,
        _ => return Err(no_update_error()),
    };

    let bytes = download_with_progress(app, &update).await?;
    app.state::<DownloadedUpdate>().store(StagedUpdate {
        update: update.clone(),
        bytes,
    });
    Ok(UpdateDownloadOutcome {
        version: update.version,
    })
}

#[cfg(desktop)]
pub async fn install_pending_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> ledger_infra::error::Result<()> {
    use tauri::Manager;

    create_pre_update_backup(app).await?;

    let slot = app.state::<DownloadedUpdate>();
    let Some(StagedUpdate { update, bytes }) = slot.take() else {
        return Err(not_downloaded_error());
    };
    let version = update.version.clone();
    if let Err(error) = update.install(&bytes) {
        slot.store(StagedUpdate { update, bytes });
        let message = error.to_string();
        tracing::warn!(error = %message, "更新安装失败，工件已放回暂存可重试");
        return Err(install_failed_error());
    }
    tracing::info!(version = %version, "更新安装完成，重启应用");
    app.restart()
}

#[cfg(test)]
mod surface_tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_surface_branches_on_appimage_env() {
        assert_eq!(
            update_surface(Some(std::ffi::OsStr::new("/opt/App.OpenLedger.AppImage"))),
            UpdateSurface::Updater
        );
        assert_eq!(update_surface(None), UpdateSurface::ReleasePage);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn desktop_non_linux_surface_is_updater() {
        assert_eq!(update_surface(None), UpdateSurface::Updater);
        assert_eq!(
            update_surface(Some(std::ffi::OsStr::new("anything"))),
            UpdateSurface::Updater
        );
    }

    #[cfg(mobile)]
    #[test]
    fn mobile_surface_is_release_page() {
        assert_eq!(update_surface(None), UpdateSurface::ReleasePage);
    }
}
