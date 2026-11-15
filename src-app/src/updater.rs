//! 应用更新器壳层机制（issue #2134 / ADR-0124 交互语义沿袭 / ADR-0145 修订注记
//! 2026-11-15 翻案重建）：nsh-updater 规则 crate 直调 + 自备 HTTP 传输。
//!
//! **归位**（ADR-0124 决策 7 / ADR-0111）：壳层机制，不持账本语义，不进基础
//! 设施、不立业务域 crate；唯一跨界是安装前调用备份域公开接缝
//! `create_pre_update_backup`（壳 → 域合法直呼）。
//!
//! **链路**（spike 实证，nsh-updater 0.5.1 registry 源码 + 官方
//! docs/app-updates.md「Rust apps」段）：包资产 `nsh/assets/nsh-updater.json`（由
//! `nsh package` 按 `nsh.json` updater 键写入，更新源打包时固化、调用方不能指定）
//! 经 [`Config::parse`] 校验，自备 HTTP 取 `latest.json` 与 `latest.json.sig` 后由
//! [`Config::check`] 验签并核对 id、版本（只升不降）、HTTPS 与平台条目；流式下载由
//! [`Download`] 做 SHA-256 与 size 双校验；随后更新前备份（备份域接缝），
//! `remember_program` + [`Installation::install`] 整目录换装，
//! `nsh_core::element::desktop::restart()` 重启（退出语义同 `Application.restart()`：
//! 新版本在本运行退出后启动，两版本永不并存）。
//!
//! **交互**（ADR-0124 决策 4 全询问式，活体对照 Web 参照前端
//! `UpdateSettings.vue` / `AppUpdateModal.vue`）：启动延迟 15s 静默检查（失败
//! 静默）+ 设置页手动检查（失败就地报错）；发现新版本走提醒弹层（含发布说明）
//! → 确认 → 下载（带进度）→ 确认后安装并重启；永不静默下载、永不静默安装；
//! 每个新版本至多自动提醒一次（「已提醒版本」记忆），手动检查不受记忆限制。
//! 自动检查开关与提醒记忆是设备本机偏好（ADR-0124 决策 8 轻量设置项，NSH 平
//! 台存储，不进 SQLite、不随 Backup/Restore 迁移）；单 stable 通道（GitHub
//! latest release 端点天然跳过 draft 与 prerelease，ADR-0124 决策 9）。
//!
//! **替身回路**（headless，ADR-0087 壳行为归 `cargo test -p nsh-app`）：传输与换装
//! 两处接缝槽（[`set_transport_for_tests`] / [`set_installer_for_tests`]）与配置字节
//! 槽（[`set_config_bytes_for_tests`]）注入替身；启动延迟经 nsh-testing 手动时钟
//! `fast_forward` 推进（不旁路睡眠）；PreUpdateBackup 触发接线含「删除即变红」
//! 负向条目（ADR-0087）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ledger_infra::error::AppError;
use nsh_base::button;
use nsh_core::prelude::*;
use nsh_core::storage::Storage;
use nsh_updater::install::Installation;
use nsh_updater::{Available, Config, Download};

use crate::backup_entry::APP_VERSION;
use crate::design::font_size;
use crate::{modal, settings, views};

// ---------------------------------------------------------------------------
// 词表（更新器用户可见文案 zh 原样承载；码化错误沿旧壳 update.* 闭集，ADR-0050）。
// ---------------------------------------------------------------------------
const CARD_TITLE: &str = "应用更新";
const CURRENT_VERSION_LABEL: &str = "当前版本";
const CHECK_BUTTON: &str = "检查更新";
const CHECKING_TEXT: &str = "正在检查更新…";
const UP_TO_DATE_TEXT: &str = "已是最新版本";
const AVAILABLE_PREFIX: &str = "发现新版本 ";
const DOWNLOAD_BUTTON: &str = "下载新版本";
const AUTO_CHECK_LABEL: &str = "启动时自动检查更新";
const AUTO_CHECK_HINT: &str =
    "开启后每次启动稍作延迟静默检查一次；发现新版本会弹层提醒，不会自动下载或安装。";
const FLOW_DOWNLOADING_PREFIX: &str = "正在下载新版本 ";
const FLOW_DOWNLOADED_TEXT: &str = "新版本已下载，可在弹层中确认安装";
const FLOW_INSTALLING_TEXT: &str = "正在备份并安装，即将重启…";
const FLOW_ERROR_PREFIX: &str = "更新失败：";
const RESUME_BUTTON: &str = "查看进度";
const RESUME_INSTALL_BUTTON: &str = "继续安装";
const MODAL_TITLE_REMIND_PREFIX: &str = "发现新版本 ";
const MODAL_TITLE_DOWNLOADING: &str = "正在下载更新";
const MODAL_TITLE_DOWNLOADED: &str = "准备安装更新";
const MODAL_LATER: &str = "以后再说";
const MODAL_DOWNLOAD_NOW: &str = "下载";
const INSTALL_WARNING: &str = "确认后应用将退出并安装新版本，完成后自动重新启动。";
const INSTALL_BACKUP_NOTE: &str = "安装前会自动为当前账本创建更新前备份；备份失败则中止更新。";
const INSTALL_BUTTON: &str = "安装并重启";
const INSTALLING_BUTTON: &str = "正在安装…";
const NO_NOTES: &str = "（本版本没有发布说明）";

/// 清单或配置不可解析（配置损坏、清单 id/版本非法、平台条目缺失）。
const CODE_MANIFEST_INVALID: &str = "update.manifest-invalid";
/// 更新源不可达（网络失败、HTTP 非 2xx、TLS 握手失败）。
const CODE_UNREACHABLE: &str = "update.unreachable";
/// 传输中断（读中途断开、本地落盘失败）。
const CODE_DOWNLOAD_FAILED: &str = "update.download-failed";
/// 工件与清单不符（SHA-256 或大小不匹配）——清单是签名保护的，等同拒绝安装。
const CODE_CHECKSUM_MISMATCH: &str = "update.checksum-mismatch";
/// 更新前备份失败——中止更新，不带病升级（ADR-0124 决策 6）。
const CODE_BACKUP_FAILED: &str = "update.backup-failed";
/// 换装失败（解包校验不过 / 目录不可写 / 本运行非打包安装形态）。
const CODE_INSTALL_FAILED: &str = "update.install-failed";

const ERR_MANIFEST_INVALID: &str = "更新源返回了无法解析的更新清单";
const ERR_UNREACHABLE: &str = "更新源暂时不可用，请检查网络后重试";
const ERR_DOWNLOAD_FAILED: &str = "更新下载失败，请检查网络后重试";
const ERR_CHECKSUM_MISMATCH: &str = "更新工件校验失败（与清单的 SHA-256 或大小不符），已拒绝安装";
const ERR_BACKUP_FAILED: &str = "更新前备份创建失败，已中止更新";
const ERR_INSTALL_FAILED: &str = "更新安装失败，请重试";
const ERR_NOT_PACKAGED: &str = "本运行不是打包安装形态，无法换装";

/// 启动延迟静默检查的延迟（旧壳 `STARTUP_CHECK_DELAY` 同值沿袭，ADR-0124 决策 4）。
const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(15);
/// 下载进度的信号刷新节流（旧壳 `PROGRESS_EMIT_INTERVAL` 同值：100ms 一次，
/// 信号只在引擎线程置位——阻塞线程侧写共享槽，引擎侧轮询读）。
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);
/// 进度轮询周期（引擎任务定时器读阻塞线程写的共享槽）。
const PROGRESS_POLL_INTERVAL: Duration = Duration::from_millis(100);

// 设备级轻量设置项键（ADR-0017 / ADR-0124 决策 8：NSH 平台存储，不与旧壳共享）。
/// 自动检查开关（缺省开启——新装默认开启自动检查）。
const AUTO_CHECK_KEY: &str = "update.auto-check";
/// 已提醒版本记忆（值 = 版本号原文；每版本至多自动提醒一次）。
const REMINDED_KEY: &str = "update.reminded-version";

/// 更新器下载工件的暂存目录名（应用数据目录下；`.part` 半成品 + 终名两段式，
/// 与 nsh-core JS 面的 updates 目录两段式同形）。
const UPDATES_DIR: &str = "updates";

// ---------------------------------------------------------------------------
// 配置接缝：包资产 nsh-updater.json（更新源在打包时固化）。
// ---------------------------------------------------------------------------

/// 配置缓存槽（进程级一次读取；测试替身直接置字节，复位点清空）。
static CONFIG_BYTES: Mutex<Option<Vec<u8>>> = Mutex::new(None);

fn config_slot() -> std::sync::MutexGuard<'static, Option<Vec<u8>>> {
    CONFIG_BYTES.lock().unwrap_or_else(|e| e.into_inner())
}

/// 读包资产字节（生产路径：可执行文件同级 `nsh/assets/nsh-updater.json`，
/// 与 `nsh package` 的 Linux 包布局及 [`Installation::current`] 的比对路径同
/// 一单点；缺文件 = 未打包运行，更新面不可用）。
fn read_config_bytes_from_package() -> Option<Vec<u8>> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    std::fs::read(
        dir.join("nsh")
            .join("assets")
            .join(nsh_updater::CONFIG_ASSET),
    )
    .ok()
}

/// 更新器配置（解析即校验：恰好一个更新源、url 源必须带公钥、版本可比较）。
pub(crate) struct UpdaterSetup {
    /// 解析后的配置（检查与换装共用）。
    pub config: Config,
    /// 原始字节（[`Installation::current`] 比对包内文件用——必须逐字节一致）。
    pub bytes: Vec<u8>,
}

/// 当前运行的更新器配置（None = 更新面不可用：开发运行、未打包、配置损坏）。
/// 进程级缓存一次；失败（含损坏）同样缓存——不反复读盘重试。
pub(crate) fn updater_setup() -> Option<UpdaterSetup> {
    let mut slot = config_slot();
    if slot.is_none() {
        *slot = read_config_bytes_from_package();
    }
    let bytes = slot.clone()?;
    match Config::parse(&bytes) {
        Ok(config) => Some(UpdaterSetup { config, bytes }),
        // 配置损坏与「没有配置」同样不可用：缓存空字节标记避免反复解析。
        Err(_) => {
            *slot = None;
            slot.replace(Vec::new());
            None
        }
    }
}

/// 置配置字节替身（headless 接缝；`None` = 复位回生产读盘路径）。
#[doc(hidden)]
pub fn set_config_bytes_for_tests(bytes: Option<Vec<u8>>) {
    *config_slot() = bytes;
}

// ---------------------------------------------------------------------------
// 传输接缝：nsh-updater 不带网络，清单 / 签名 / 工件字节由本壳自备 HTTP。
// ---------------------------------------------------------------------------

/// 分片接收器：收一段字节与总长（已知时）；返回 Err 即中止传输。
pub type ByteSink<'a> = &'a mut dyn FnMut(&[u8], Option<u64>) -> Result<(), String>;

/// 更新源字节获取（生产 = reqwest blocking + rustls；headless = 内存替身）。
/// `sink` 分片收字节（`total` 总长已知时随片给出）；返回 Err 即传输失败。
pub trait Transport: Send + Sync + 'static {
    /// 小载荷（清单、分离签名）：总超时兜底——拿不全的清单连接早点失败。
    fn get_bounded(&self, url: &str, sink: ByteSink<'_>) -> Result<(), String>;

    /// 大载荷（工件）：连接超时兜底、无总时长（几十 MB 慢链路下载不误杀）。
    fn get(&self, url: &str, sink: ByteSink<'_>) -> Result<(), String>;
}

/// 传输槽（缺省 = HTTP 传输；测试替身经 [`set_transport_for_tests`] 换装）。
static TRANSPORT: Mutex<Option<Arc<dyn Transport>>> = Mutex::new(None);

fn transport() -> Arc<dyn Transport> {
    // clone 不 take：桩在多次调用（检查腿 + 下载腿）间保持；空槽 = 生产缺省。
    transport_slot()
        .clone()
        .unwrap_or_else(|| Arc::new(HttpTransport::default()))
}

fn transport_slot() -> std::sync::MutexGuard<'static, Option<Arc<dyn Transport>>> {
    TRANSPORT.lock().unwrap_or_else(|e| e.into_inner())
}

/// 置传输替身（headless 接缝；`None` = 复位回生产 HTTP 传输）。
#[doc(hidden)]
pub fn set_transport_for_tests(transport: Option<Arc<dyn Transport>>) {
    *transport_slot() = transport;
}

/// 生产 HTTP 传输：blocking 客户端（跑 [`blocking`] 任务线程），TLS 走 rustls
///（webpki roots 兼 GitHub 证书链）；检查载荷总超时 10s（旧壳 CHECK_TIMEOUT
/// 同值沿袭），工件下载无总时长（连接超时兜底，几十 MB 慢链路不误杀）。
struct HttpTransport {
    /// 清单/签名客户端（总超时）。
    check_client: reqwest::blocking::Client,
    /// 工件客户端（连接超时、无总时长）。
    download_client: reqwest::blocking::Client,
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self {
            check_client: reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            download_client: reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Transport for HttpTransport {
    fn get_bounded(&self, url: &str, sink: ByteSink<'_>) -> Result<(), String> {
        self.get_with(&self.check_client, url, sink)
    }

    fn get(&self, url: &str, sink: ByteSink<'_>) -> Result<(), String> {
        self.get_with(&self.download_client, url, sink)
    }
}

impl HttpTransport {
    /// 分片拉取（跟随重定向到平台缺省上限；内容长度随首片给出）。
    fn get_with(
        &self,
        client: &reqwest::blocking::Client,
        url: &str,
        sink: ByteSink<'_>,
    ) -> Result<(), String> {
        let response = client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?;
        let total = response.content_length();
        let mut reader = response;
        let mut buf = [0u8; 16 * 1024];
        loop {
            let read = std::io::Read::read(&mut reader, &mut buf).map_err(|e| e.to_string())?;
            if read == 0 {
                return Ok(());
            }
            sink(&buf[..read], total)?;
        }
    }
}

// ---------------------------------------------------------------------------
// 换装执行接缝：备份成功后的最后三步（remember_program → install → restart）。
// ---------------------------------------------------------------------------

/// 换装执行器（生产 = 真实安装链；headless 替身只记录，永不触碰真实目录）。
pub(crate) type Installer = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// 换装执行器槽（缺省 = 生产链；headless 经 [`set_installer_for_tests`] 换装）。
static INSTALLER: Mutex<Option<Installer>> = Mutex::new(None);

fn installer_slot() -> std::sync::MutexGuard<'static, Option<Installer>> {
    INSTALLER.lock().unwrap_or_else(|e| e.into_inner())
}

/// 置换装执行器替身（headless 接缝；`None` = 复位回生产链）。
#[doc(hidden)]
pub fn set_installer_for_tests(installer: Option<Installer>) {
    *installer_slot() = installer;
}

/// 生产换装链（nsh-core JS 面 `install` 的同序对应物）：定位本应用的安装
///（可执行旁 app.json 同 id + nsh-updater.json 同字节，非打包运行 None）→
/// 记住运行程序路径（换装后本进程可执行即被移走的旧文件）→ 整目录换装 →
/// 重启（退出语义同 `Application.restart()`，新版本在本进程退出后才启动）。
fn perform_install(setup: &UpdaterSetup, archive: &Path) -> Result<(), String> {
    let installation =
        Installation::current(&setup.config.id, &setup.bytes).ok_or(ERR_NOT_PACKAGED.to_owned())?;
    // 换装改盘前记住程序路径：重启启动用户认识的那条路径（JS 面同注释语义）。
    nsh_core::restart::remember_program();
    installation.install(archive).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(archive);
    nsh_core::element::desktop::restart();
    Ok(())
}

// ---------------------------------------------------------------------------
// 流程态与存储项。
// ---------------------------------------------------------------------------

/// 一个可执行的更新（[`Config::check`] 通过后的产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlanUpdate {
    /// 新版本号。
    pub version: String,
    /// 发布说明（提醒弹层展示；None = 清单未写）。
    pub notes: Option<String>,
    /// 工件下载地址（已按清单基址解析为绝对 URL）。
    pub url: String,
    /// 工件 SHA-256（hex；None = 清单给的是引导页而非工件——单 stable 直发
    /// 形态不出现，出现即按清单不可用拒绝）。
    pub sha256: Option<String>,
    /// 工件字节数。
    pub size: Option<u64>,
}

impl From<Available> for PlanUpdate {
    fn from(available: Available) -> Self {
        Self {
            version: available.version,
            notes: available.notes,
            url: available.url,
            sha256: available.sha256,
            size: available.size,
        }
    }
}

/// 下载进度快照（阻塞线程写、引擎线程读的槽内值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DownloadProgress {
    /// 已接收字节数。
    pub received: u64,
    /// 总字节数（清单或响应头给出；None = 未知，进度条走 processing 态）。
    pub total: Option<u64>,
}

/// 更新流程态（全局一场——模块单例，与 Web 参照前端 useAppUpdate 的模块级
/// 单例 ref 同形；Idle 之外的每态对应弹层或卡片的一个阶段）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Flow {
    /// 无进行中流程。
    Idle,
    /// 已发现新版本，待用户确认下载（提醒弹层开着）。
    Remind(PlanUpdate),
    /// 下载中（进度快照；弹层可收起，下载在后台继续）。
    Downloading(PlanUpdate, DownloadProgress),
    /// 已下载待安装（弹层开着呈安装确认；收起后卡片流程行再入口）。
    Downloaded(PlanUpdate),
    /// 备份 + 换装在途（弹层钉住不可关——本进程即将退出）。
    Installing(PlanUpdate),
}

/// 手动检查态（设置卡就地三态；与流程态独立——手动检查不弹层、不写提醒记忆）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CheckState {
    /// 未检查。
    Idle,
    /// 检查中（按钮禁用挡重复）。
    Checking,
    /// 已是最新。
    UpToDate,
    /// 发现新版本（就地呈现版本号 + 发布说明 + 下载入口）。
    Available(PlanUpdate),
    /// 检查失败（码化文案就地报错）。
    Failed(String),
}

/// 更新器反应式状态句柄（Copy；信号挂首次 [`store`] 调用所在组件实例——
/// 生产契约：`App` 组件体调用，状态挂根实例随进程存活，nav store 同纪律）。
#[derive(Clone, Copy)]
pub(crate) struct UpdateStore {
    /// 初始化时捕获的根 owner（`App` 组件体首调 [`store`] 的现场）：动作任务
    /// 的挂载点——弹层内容是 dynamic 区域，区域重运行即换 owner，直接
    /// `Owner::current()` 会把任务挂到随即销毁的 run owner 上（视图装载竞态
    /// 先例同形），任务必须挂根实例。
    pub owner: Owner,
    /// 手动检查态。
    pub check: Signal<CheckState>,
    /// 流程态（全局一场）。
    pub flow: Signal<Flow>,
    /// 流程内失败（弹层错误位 + 卡片流程行就地呈现；None = 无错）。
    pub flow_error: Signal<Option<String>>,
    /// 弹层开合（与流程态分离：下载中/已下载收起弹层不中断流程，卡片流程行
    /// 再入口；提醒阶段关闭 = 取消不下载）。
    pub modal_open: Signal<bool>,
}

/// 进程级更新器状态槽（headless 多场景串行复位；锁毒化取回内值——槽位本身
/// 不含不变量，nav 槽同形）。
static STORE: Mutex<Option<UpdateStore>> = Mutex::new(None);

fn store_slot() -> std::sync::MutexGuard<'static, Option<UpdateStore>> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// 更新器状态（惰性初始化，须在引擎线程调用；生产路径在 `App` 组件体）。
pub(crate) fn store() -> UpdateStore {
    let mut slot = store_slot();
    if let Some(s) = *slot {
        return s;
    }
    let s = UpdateStore {
        owner: Owner::current(),
        check: signal(CheckState::Idle),
        flow: signal(Flow::Idle),
        flow_error: signal(None),
        modal_open: signal(false),
    };
    *slot = Some(s);
    s
}

/// 复位更新器现场（headless 测试接缝，[`crate::app::reset_book_assembly`] 消费）：
/// 状态槽与三个替身/配置槽同批复位（上一场景的传输桩 / 换装桩 / 配置字节不得
/// 泄入下一场景）。
#[doc(hidden)]
pub fn reset_for_tests() {
    *store_slot() = None;
    *transport_slot() = None;
    *installer_slot() = None;
    *config_slot() = None;
    STARTUP_SCHEDULED.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// 读自动检查开关（缺省开启——新装默认开启自动检查，ADR-0124 决策 8）。
fn auto_check_enabled() -> bool {
    Storage::get_item(AUTO_CHECK_KEY)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// 写自动检查开关（设备级轻量设置项；写失败不静默，告警留痕）。
pub(crate) fn set_auto_check(enabled: bool) {
    let value = serde_json::json!(enabled).to_string();
    if let Err(e) = Storage::set_item(AUTO_CHECK_KEY, value) {
        tracing::warn!(key = AUTO_CHECK_KEY, error = %e, "自动检查开关写入存储失败");
    }
}

/// 读已提醒版本记忆。
fn reminded_version() -> Option<String> {
    Storage::get_item(REMINDED_KEY)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|value| value.as_str().map(str::to_owned))
}

/// 写已提醒版本记忆（每个新版本至多自动提醒一次的落点；失败告警留痕——
/// 下一启动重复提醒一次，不丢数据）。
fn write_reminded_version(version: &str) {
    let value = serde_json::json!(version).to_string();
    if let Err(e) = Storage::set_item(REMINDED_KEY, value) {
        tracing::warn!(key = REMINDED_KEY, error = %e, "已提醒版本写入存储失败");
    }
}

// ---------------------------------------------------------------------------
// 检查（静默 / 手动同一取数面，呈现与记忆语义分叉）。
// ---------------------------------------------------------------------------

/// nsh-updater 错误 → 码化用户可见文案（旧壳 update.* 闭集 + checksum 新码，
/// ADR-0050）。
fn coded_error(error: nsh_updater::Error) -> AppError {
    match &error {
        nsh_updater::Error::Config(_) | nsh_updater::Error::Manifest(_) => {
            AppError::coded(CODE_MANIFEST_INVALID, ERR_MANIFEST_INVALID)
        }
        nsh_updater::Error::Download(_) => {
            AppError::coded(CODE_CHECKSUM_MISMATCH, ERR_CHECKSUM_MISMATCH)
        }
        nsh_updater::Error::Install(_) => AppError::coded(CODE_INSTALL_FAILED, ERR_INSTALL_FAILED),
        nsh_updater::Error::Io(_) => AppError::coded(CODE_DOWNLOAD_FAILED, ERR_DOWNLOAD_FAILED),
    }
}

/// 传输失败 → 码化「更新源暂时不可用」（清单与签名两腿共用）。
fn unreachable_error() -> AppError {
    AppError::coded(CODE_UNREACHABLE, ERR_UNREACHABLE)
}

/// 取清单 + 分离签名并过 [`Config::check`]（静默与手动的同一取数面；签名腿
/// 仅在配置带公钥时取）。
fn fetch_and_check(setup: &UpdaterSetup) -> Result<Option<PlanUpdate>, AppError> {
    let transport = transport();
    let mut manifest = Vec::new();
    transport
        .get_bounded(&setup.config.manifest_url(), &mut |chunk, _| {
            manifest.extend_from_slice(chunk);
            Ok(())
        })
        .map_err(|_| unreachable_error())?;
    let mut signature = None;
    if let Some(sig_url) = setup.config.signature_url() {
        let mut sig = Vec::new();
        transport
            .get_bounded(&sig_url, &mut |chunk, _| {
                sig.extend_from_slice(chunk);
                Ok(())
            })
            .map_err(|_| unreachable_error())?;
        signature = Some(sig);
    }
    setup
        .config
        .check(&manifest, signature.as_deref(), &nsh_updater::platform())
        .map(|available| available.map(PlanUpdate::from))
        .map_err(coded_error)
}

/// 启动延迟静默检查（ADR-0124 决策 4 后台路径）：未配置 / 开关关闭直接返回；
/// 失败静默（tracing 留痕）；发现新版本且非已提醒版本才弹层，弹层同时写记忆
///——同版本后续启动不再打扰。
fn silent_check() {
    let Some(setup) = updater_setup() else {
        return;
    };
    if !auto_check_enabled() {
        return;
    }
    let store = store();
    store.owner.run(move || {
        spawn(async move {
            let outcome = blocking(move || fetch_and_check(&setup)).await;
            let plan = match outcome {
                Ok(Ok(Some(plan))) => plan,
                Ok(Ok(None)) => return,
                Ok(Err(error)) => {
                    tracing::warn!(error = %error, "后台更新检查失败（静默）");
                    return;
                }
                Err(_) => {
                    tracing::warn!("后台更新检查任务异常退出（静默）");
                    return;
                }
            };
            // 已提醒版本记忆：每个新版本至多自动提醒一次；流程进行中不重弹。
            if reminded_version().as_deref() == Some(plan.version.as_str())
                || !matches!(store.flow.get_untracked(), Flow::Idle)
            {
                return;
            }
            write_reminded_version(&plan.version);
            store.flow_error.set(None);
            store.flow.set(Flow::Remind(plan));
            store.modal_open.set(true);
        });
    });
}

/// 启动延迟静默检查的排程旗标（进程级一次——多会话重复就绪不重复排程；
/// headless 复位点随 [`reset_for_tests`]）。
static STARTUP_SCHEDULED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 启动延迟静默检查的调度接线（`App` 就绪分支单点调用；nsh-testing 手动时钟
/// `fast_forward` 可推进，headless 删除本接线即「启动检查探针」红）。
pub(crate) fn schedule_startup_check() {
    if STARTUP_SCHEDULED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    set_timeout(STARTUP_CHECK_DELAY, silent_check);
}

/// 手动检查（设置卡入口）：检查中禁用挡重复；发现新版本就地呈现（不弹层、
/// 不写提醒记忆——ADR-0124 决策 4「手动检查不受该记忆限制」的写侧对应）；
/// 失败就地报错（码化文案）。
pub(crate) fn check_manual(store: UpdateStore) {
    if store.check.get_untracked() == CheckState::Checking {
        return;
    }
    let Some(setup) = updater_setup() else {
        store.check.set(CheckState::Failed(
            AppError::coded(CODE_INSTALL_FAILED, ERR_NOT_PACKAGED).to_string(),
        ));
        return;
    };
    store.check.set(CheckState::Checking);
    store.owner.run(move || {
        spawn(async move {
            let outcome = blocking(move || fetch_and_check(&setup)).await;
            match outcome {
                Ok(Ok(None)) => store.check.set(CheckState::UpToDate),
                Ok(Ok(Some(plan))) => store.check.set(CheckState::Available(plan)),
                Ok(Err(error)) => store.check.set(CheckState::Failed(error.to_string())),
                Err(_) => store.check.set(CheckState::Failed(
                    "内部错误：更新检查任务异常退出".to_owned(),
                )),
            }
        });
    });
}

// ---------------------------------------------------------------------------
// 下载与安装。
// ---------------------------------------------------------------------------

/// 阻塞线程侧的下载实时槽（进度节流写、引擎轮询读——信号只在引擎线程置位）。
#[derive(Default)]
struct LiveDownload {
    /// 最近一次快照（节流后的）。
    progress: Option<DownloadProgress>,
}

/// 更新工件落盘路径（应用数据目录 `updates/ledger-<version>.tar.gz`；数据根
/// 未注入 = 码化拒绝——下载无落点比静默丢失诚实）。
fn archive_target(version: &str) -> Result<PathBuf, AppError> {
    let dir = crate::cross_book::default_data_dir()
        .ok_or_else(|| AppError::coded(CODE_DOWNLOAD_FAILED, ERR_DOWNLOAD_FAILED))?
        .join(UPDATES_DIR);
    Ok(dir.join(format!("ledger-{version}.tar.gz")))
}

/// 阻塞线程上的下载执行（[`Download`] 流式 SHA-256 + size 双校验，`.part`
/// 半成品两段式；进度节流写共享槽）。返回终件路径。
fn run_download(
    transport: Arc<dyn Transport>,
    plan: &PlanUpdate,
    target: &Path,
    live: &Mutex<LiveDownload>,
) -> Result<PathBuf, String> {
    // 无校验值 = 清单给的是引导页（商店形态）——单 stable 直发不出现，拒绝
    // 比静默装一个未知字节集诚实。
    let Some(sha256) = plan.sha256.as_deref() else {
        return Err(AppError::coded(CODE_MANIFEST_INVALID, ERR_MANIFEST_INVALID).to_string());
    };
    let dir = target
        .parent()
        .ok_or_else(|| AppError::coded(CODE_DOWNLOAD_FAILED, ERR_DOWNLOAD_FAILED).to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = target.with_file_name(format!(
        "{}.part",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive")
    ));
    // 半途而废的旧半成品不续传：删掉重来（nsh-core JS 面同语义）。
    let _ = std::fs::remove_file(&part);
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut download = Download::new(sha256, plan.size);
    let mut last_emit = Instant::now() - PROGRESS_EMIT_INTERVAL;
    transport
        .get(&plan.url, &mut |chunk, total| {
            let received = download.update(chunk).map_err(|_| {
                AppError::coded(CODE_CHECKSUM_MISMATCH, ERR_CHECKSUM_MISMATCH).to_string()
            })?;
            file.write_all(chunk).map_err(|e| e.to_string())?;
            if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
                last_emit = Instant::now();
                let mut guard = live.lock().unwrap_or_else(|e| e.into_inner());
                guard.progress = Some(DownloadProgress {
                    received,
                    total: plan.size.or(total),
                });
            }
            Ok(())
        })
        .map_err(|_| AppError::coded(CODE_UNREACHABLE, ERR_UNREACHABLE).to_string())?;
    download
        .finish()
        .map_err(|_| AppError::coded(CODE_CHECKSUM_MISMATCH, ERR_CHECKSUM_MISMATCH).to_string())?;
    std::fs::rename(&part, target).map_err(|e| e.to_string())?;
    Ok(target.to_path_buf())
}

/// 开始下载（提醒弹层「下载」与设置卡「下载新版本」的唯一写面；单飞——流程
/// 进行中重入直接忽略）。弹层转下载态；失败就地回提醒态（工件半成品已清，
/// 重试从头下载）。
pub(crate) fn start_download(store: UpdateStore, plan: PlanUpdate) {
    if !matches!(store.flow.get_untracked(), Flow::Idle | Flow::Remind(_)) {
        return;
    }
    let target = match archive_target(&plan.version) {
        Ok(target) => target,
        Err(error) => {
            store.flow_error.set(Some(
                AppError::coded(CODE_DOWNLOAD_FAILED, ERR_DOWNLOAD_FAILED).to_string(),
            ));
            tracing::warn!(%error, "更新工件落盘路径解析失败");
            return;
        }
    };
    store.flow_error.set(None);
    store.modal_open.set(true);
    store.flow.set(Flow::Downloading(
        plan.clone(),
        DownloadProgress {
            received: 0,
            total: plan.size,
        },
    ));
    store.owner.run(move || {
        spawn(async move {
            let live = Arc::new(Mutex::new(LiveDownload::default()));
            let transport = transport();
            let download_plan = plan.clone();
            let live_ref = Arc::clone(&live);
            let handle =
                blocking(move || run_download(transport, &download_plan, &target, &live_ref));
            // 进度轮询（阻塞线程节流写共享槽，引擎线程 100ms 读一次置信号）。
            loop {
                if handle.is_finished() {
                    break;
                }
                timer(PROGRESS_POLL_INTERVAL).await;
                let snapshot = live.lock().unwrap_or_else(|e| e.into_inner()).progress;
                if let Some(progress) = snapshot {
                    store.flow.set(Flow::Downloading(plan.clone(), progress));
                }
            }
            match handle.await {
                Ok(Ok(_archive)) => {
                    store.flow.set(Flow::Downloaded(plan));
                }
                Ok(Err(message)) => {
                    store.flow_error.set(Some(message));
                    store.flow.set(Flow::Remind(plan));
                }
                Err(_) => {
                    store
                        .flow_error
                        .set(Some("内部错误：下载任务异常退出".to_owned()));
                    store.flow.set(Flow::Remind(plan));
                }
            }
        });
    });
}

/// 更新前备份（安装编排第 1 步，安装确认后、旧版退出前，ADR-0124 决策 6）：
/// 备份域 `create_pre_update_backup` 经写连接锁**裸作业**投递（不经统一写
/// 入口——不置脏、调度状态零接触，先例同 `boot::create_backup`）；目录 = 应
/// 用数据目录（与旧壳 `app_data_dir()` 同语义单点，独立于用户备份目录——备份
/// 域 PreUpdateBackup 词条口径）；失败原样上抛由编排中止更新——不带病升级。
fn create_pre_update_backup_for_install() -> Result<(), String> {
    let dir = crate::cross_book::default_data_dir()
        .ok_or_else(|| AppError::coded(CODE_BACKUP_FAILED, ERR_BACKUP_FAILED).to_string())?;
    let Some(db) = crate::app::db_slot().clone() else {
        return Err(AppError::coded(CODE_BACKUP_FAILED, ERR_BACKUP_FAILED).to_string());
    };
    let conn = db.conn.lock().map_err(|e| {
        AppError::coded(CODE_BACKUP_FAILED, format!("{ERR_BACKUP_FAILED}（{e}）")).to_string()
    })?;
    ledger_backup::create_pre_update_backup(&conn, &dir, APP_VERSION)
        .map(|_| ())
        .map_err(|error| {
            tracing::error!(%error, "更新前备份创建失败，中止更新");
            AppError::coded(CODE_BACKUP_FAILED, ERR_BACKUP_FAILED).to_string()
        })
}

/// 确认安装（下载完成态的显式二次确认入口，全询问式最后一道）：更新前备份
///（失败即中止，已下载工件保留可重试）→ 换装链（[`perform_install`]：成功
/// 不返回——进程经 `restart()` 退出；失败回已下载态就地报错）。
pub(crate) fn confirm_install(store: UpdateStore) {
    let Flow::Downloaded(plan) = store.flow.get_untracked() else {
        return;
    };
    store.flow_error.set(None);
    store.flow.set(Flow::Installing(plan.clone()));
    store.owner.run(move || {
        spawn(async move {
            // 第 1 步：更新前备份（阻塞线程裸作业；失败即中止——不带病升级）。
            if let Err(message) = blocking(create_pre_update_backup_for_install)
                .await
                .unwrap_or_else(|_| {
                    Err(AppError::coded(CODE_BACKUP_FAILED, ERR_BACKUP_FAILED).to_string())
                })
            {
                store.flow_error.set(Some(message));
                store.flow.set(Flow::Downloaded(plan));
                return;
            }
            // 第 2 步：换装链（成功即重启不返回；失败回已下载态就地报错，
            // 应用保持原样——官方语义「If install fails, the app stays as
            // it was」）。
            let setup = updater_setup();
            let install_plan = plan.clone();
            let outcome = match setup {
                Some(setup) => {
                    let installer = installer_slot().clone();
                    blocking(move || match installer {
                        Some(installer) => installer(&install_plan_archive(&install_plan)),
                        None => perform_install(&setup, &install_plan_archive(&install_plan)),
                    })
                    .await
                }
                None => Ok(Err(ERR_NOT_PACKAGED.to_owned())),
            };
            match outcome {
                Ok(Ok(())) => {
                    // 成功路径不返回（生产 restart 已触发进程退出）；替身回路
                    //（headless）停在安装态，备份在场即断言面。
                    store.flow.set(Flow::Installing(plan));
                }
                Ok(Err(message)) => {
                    store
                        .flow_error
                        .set(Some(format!("{ERR_INSTALL_FAILED}（{message}）")));
                    store.flow.set(Flow::Downloaded(plan));
                }
                Err(_) => {
                    store.flow_error.set(Some(
                        AppError::coded(CODE_INSTALL_FAILED, ERR_INSTALL_FAILED).to_string(),
                    ));
                    store.flow.set(Flow::Downloaded(plan));
                }
            }
        });
    });
}

/// 流程内计划对应的已下载工件路径（安装编排消费；与 [`archive_target`] 同一
/// 命名单点）。
fn install_plan_archive(plan: &PlanUpdate) -> PathBuf {
    archive_target(&plan.version).unwrap_or_else(|_| PathBuf::from("ledger-update.tar.gz"))
}

// ---------------------------------------------------------------------------
// 弹层与设置卡。
// ---------------------------------------------------------------------------

/// 关闭弹层（✕ / ESC 的统一出口，按阶段分流——ADR-0124 全询问式关闭语义）：
/// 提醒阶段 = 取消不下载（流程回场）；下载中 = 只收起（下载后台继续）；
/// 已下载 = 只收起（不安装，卡片流程行再入口）；安装中不可关（备份与换装
/// 在途，本进程即将退出）。
pub(crate) fn close_modal(store: UpdateStore) {
    match store.flow.get_untracked() {
        Flow::Remind(_) => {
            store.modal_open.set(false);
            store.flow.set(Flow::Idle);
        }
        Flow::Downloading(..) | Flow::Downloaded(_) => store.modal_open.set(false),
        Flow::Installing(_) => {}
        Flow::Idle => store.modal_open.set(false),
    }
}

/// 更新弹层（App 根挂载，AppUpdateModal.vue 的 NSH 对应件）：三阶段同一弹层
/// 按 [`Flow`] 分形——提醒（版本号 + 发布说明 + 以后再说 / 下载）、下载中
///（进度条 + 收起不中断）、已下载（warning 级安装确认 + 兜底说明 + 在途禁
/// 用）；流程内失败就地渲染错误位（码化文案，不走全局 toast）。
pub(crate) fn update_modal() -> impl IntoElement {
    let store = store();
    let palette = views::view_palette();
    // open 是 tracked 读（流程态也读）：开合与阶段转换都触发重评估——闭包值
    // 只在「弹层应开」边界翻转，阶段内部切换由内容 dynamic 区域承担。
    let open = move || store.modal_open.get() && store.flow.get() != Flow::Idle;
    let title = move || match store.flow.get_untracked() {
        Flow::Remind(plan) => format!("{MODAL_TITLE_REMIND_PREFIX}{}", plan.version),
        Flow::Downloading(..) => MODAL_TITLE_DOWNLOADING.to_owned(),
        _ => MODAL_TITLE_DOWNLOADED.to_owned(),
    };
    let close = move || close_modal(store);
    let content = move || {
        // 阶段依赖的主体走 dynamic 区域（读流程态信号，阶段转换即重建——when
        // 仅条件翻转重建，不适用同开弹层内的阶段切换）；child 包裹以落在
        // View 形面内。
        view().child(nsh_core::element::dynamic(move || {
            let flow = store.flow.get();
            let error = store.flow_error.get();
            let mut body = view().flex_col().app_class("gap-2");
            // 错误位（流程内失败就地呈现；码化文案原文）。
            if let Some(message) = error {
                body = body.child(
                    text(format!("{FLOW_ERROR_PREFIX}{message}"))
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.danger_fg)
                        .test_id("update-flow-error"),
                );
            }
            match flow {
                Flow::Remind(plan) => {
                    body = body
                        .child(
                            text(plan.notes.clone().unwrap_or_else(|| NO_NOTES.to_owned()))
                                .text_size(px(font_size::SECONDARY))
                                .text_color(palette.fg)
                                .test_id("update-modal-notes"),
                        )
                        .child(
                            view().flex_row().app_class("justify-end gap-2").child(
                                button()
                                    .app_class("h-8 px-3 rounded-(--radius) border border-border text-foreground hover:bg-accent")
                                    .child(text(MODAL_LATER).text_sm().test_id("update-modal-later"))
                                    .on_press(move |_: &()| close_modal(store)),
                            ),
                        )
                        .child(download_button(store, palette, plan, MODAL_DOWNLOAD_NOW));
                }
                Flow::Downloading(_, progress) => {
                    body = body.child(progress_bar(progress)).child(
                        text(progress_text(progress))
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.muted)
                            .test_id("update-modal-progress"),
                    );
                }
                Flow::Downloaded(_) => {
                    body = body
                        .child(
                            view()
                                .flex_col()
                                .app_class("rounded-sm border border-warning/30 bg-warning/10 p-3 gap-1")
                                .child(
                                    text(INSTALL_WARNING)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(palette.fg),
                                )
                                .child(
                                    text(INSTALL_BACKUP_NOTE)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(palette.muted)
                                        .test_id("update-install-backup-note"),
                                ),
                        )
                        .child(
                            view().flex_row().app_class("justify-end gap-2").child(
                                button()
                                    .disabled(move || {
                                        matches!(store.flow.get_untracked(), Flow::Installing(_))
                                    })
                                    .app_class("h-8 px-3 rounded-(--radius) bg-warning hover:opacity-90 text-warning-foreground disabled:opacity-40")
                                    .child(
                                        text(move || match store.flow.get_untracked() {
                                            Flow::Installing(_) => INSTALLING_BUTTON.to_owned(),
                                            _ => INSTALL_BUTTON.to_owned(),
                                        })
                                        .text_sm()
                                        .test_id("update-install-btn"),
                                    )
                                    .on_press(move |_: &()| {
                                        confirm_install(store);
                                    }),
                            ),
                        );
                }
                Flow::Installing(_) => {
                    body = body.child(
                        text(FLOW_INSTALLING_TEXT)
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.muted)
                            .test_id("update-installing"),
                    );
                }
                Flow::Idle => {}
            }
            body
        }))
    };
    modal::app_modal(open, title, close, content)
}

/// 下载入口按钮（提醒弹层与设置卡共用的唯一下载写面形态；文案随入口面）。
fn download_button(
    store: UpdateStore,
    palette: views::ViewPalette,
    plan: PlanUpdate,
    label: &'static str,
) -> impl IntoElement {
    button()
        .app_class(
            "h-8 px-3 rounded-(--radius) bg-primary hover:opacity-90 text-primary-foreground",
        )
        .child(
            text(label)
                .text_sm()
                .text_color(palette.fg)
                .test_id("update-download-btn"),
        )
        .on_press(move |_: &()| {
            start_download(store, plan.clone());
        })
}

/// 进度条（nsh-base Progress：已知总量按百分比、未知走 processing 态）。
fn progress_bar(progress: DownloadProgress) -> impl IntoElement {
    let ratio = move || match progress.total {
        Some(total) if total > 0 => (progress.received as f32 / total as f32) * 100.0,
        _ => 0.0,
    };
    nsh_base::progress()
        .value(ratio)
        .accessibility_label("更新下载进度")
        .app_class("h-2 rounded-sm bg-muted overflow-hidden")
        .child(view())
}

/// 进度文本（百分比或已接收字节数——总量未知时退化为字节数）。
fn progress_text(progress: DownloadProgress) -> String {
    match progress.total {
        Some(total) if total > 0 => {
            format!(
                "{} / {} MB",
                progress.received / 1_048_576,
                total / 1_048_576
            )
        }
        _ => format!("{} MB", progress.received / 1_048_576),
    }
}

/// 设置卡「应用更新」（UpdateSettings.vue 更新卡的 NSH 对应件，通用页签）：
/// 当前版本行 → 手动检查按钮（三态就地：最新 / 新版本+下载 / 失败报错）→
/// 自动检查开关 → 流程状态行（弹层收起后的再入口）。
pub(crate) fn update_card(palette: views::ViewPalette) -> impl IntoElement {
    let store = store();
    let auto = signal(auto_check_enabled());
    view().flex_col().app_class("gap-2").child(
        settings::card(
            CARD_TITLE,
            palette,
            vec![
                // 当前版本行（CARGO_PKG_VERSION 真源，与备份水印同源）。
                view()
                    .flex_row()
                    .app_class("items-center gap-2")
                    .child(
                        text(CURRENT_VERSION_LABEL)
                            .text_size(px(font_size::SECONDARY))
                            .text_color(palette.fg),
                    )
                    .child(
                        text(APP_VERSION)
                            .text_size(px(font_size::SECONDARY))
                            .text_color(palette.muted)
                            .test_id("update-current-version"),
                    ),
                // 手动检查按钮 + 三态就地结果。
                view().flex_row().app_class("items-center gap-2").child(
                    button()
                        .disabled(move || store.check.get() == CheckState::Checking)
                        .app_class("h-8 px-3 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40")
                        .child(
                            text(move || match store.check.get() {
                                CheckState::Checking => CHECKING_TEXT.to_owned(),
                                _ => CHECK_BUTTON.to_owned(),
                            })
                            .text_sm()
                            .test_id("update-check-btn"),
                        )
                        .on_press(move |_: &()| {
                            check_manual(store);
                        }),
                ),
                view().child(check_result_line(store, palette)),
                // 自动检查开关（设备级轻量设置项，读写直落平台存储）。
                view()
                    .flex_row()
                    .app_class("items-center gap-2")
                    .child(
                        nsh_base::switch()
                            .accessibility_label(AUTO_CHECK_LABEL)
                            .checked(move || auto.get())
                            .on_change(move |value: &bool| {
                                auto.set(value.to_owned());
                                set_auto_check(value.to_owned());
                            })
                            .app_class("shrink-0"),
                    )
                    .child(
                        text(AUTO_CHECK_LABEL)
                            .text_size(px(font_size::SECONDARY))
                            .text_color(palette.fg)
                            .test_id("update-auto-switch"),
                    ),
                view().child(
                    text(AUTO_CHECK_HINT)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.muted),
                ),
                // 流程状态行（弹层收起后的再入口）。
                view().child(flow_status_line(store, palette)),
            ],
        ),
    )
}

/// 手动检查结果行（三态就地：最新 / 新版本 + 下载按钮 / 失败码化报错；各态
/// 一个 when——态间转换即条件翻转，分支重建不滞后）。
fn check_result_line(store: UpdateStore, palette: views::ViewPalette) -> impl IntoElement {
    view()
        .flex_col()
        .app_class("gap-1")
        .child(when(
            move || store.check.get() == CheckState::UpToDate,
            move || {
                text(UP_TO_DATE_TEXT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted)
                    .test_id("update-check-latest")
            },
        ))
        .child(when(
            move || matches!(store.check.get(), CheckState::Available(_)),
            move || {
                let CheckState::Available(plan) = store.check.get_untracked() else {
                    return view();
                };
                view()
                    .flex_col()
                    .app_class("gap-1")
                    .child(
                        text(format!("{AVAILABLE_PREFIX}{}", plan.version))
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.fg)
                            .test_id("update-check-version"),
                    )
                    .child(
                        text(plan.notes.clone().unwrap_or_else(|| NO_NOTES.to_owned()))
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.muted),
                    )
                    .child(download_button(store, palette, plan, DOWNLOAD_BUTTON))
            },
        ))
        .child(when(
            move || matches!(store.check.get(), CheckState::Failed(_)),
            move || {
                let CheckState::Failed(message) = store.check.get_untracked() else {
                    return view();
                };
                view().child(
                    text(message)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.danger_fg)
                        .test_id("update-check-error"),
                )
            },
        ))
}

/// 流程状态行（弹层收起后的再入口：下载中百分比 / 已下载继续安装 / 失败文案；
/// 可见性条件随流程态与弹层开合翻转，分支重建不滞后）。
fn flow_status_line(store: UpdateStore, palette: views::ViewPalette) -> impl IntoElement {
    let hidden = move || match store.flow.get() {
        Flow::Idle | Flow::Remind(_) => store.flow_error.get_untracked().is_none(),
        _ => store.modal_open.get(),
    };
    when(hidden, view).otherwise(move || {
        let flow = store.flow.get_untracked();
        let error = store.flow_error.get_untracked();
        let status = match &flow {
            Flow::Downloading(plan, progress) => {
                format!("{FLOW_DOWNLOADING_PREFIX}{}：{}", plan.version, progress_text(*progress))
            }
            Flow::Downloaded(plan) => format!("{FLOW_DOWNLOADED_TEXT}（{}）", plan.version),
            Flow::Installing(_) => FLOW_INSTALLING_TEXT.to_owned(),
            Flow::Remind(_) | Flow::Idle => error
                .map(|message| format!("{FLOW_ERROR_PREFIX}{message}"))
                .unwrap_or_default(),
        };
        let mut body = view().flex_row().app_class("items-center gap-2").child(
            text(status)
                .text_size(px(font_size::CAPTION))
                .text_color(palette.muted)
                .test_id("update-card-flow-status"),
        );
        // 再入口按钮：下载中/已下载回弹层。
        if matches!(flow, Flow::Downloading(..) | Flow::Downloaded(_)) {
            body = body.child(
                button()
                    .app_class("h-7 px-2 rounded-(--radius) border border-border text-foreground hover:bg-accent")
                    .child(
                        text(move || match store.flow.get_untracked() {
                            Flow::Downloaded(_) => RESUME_INSTALL_BUTTON.to_owned(),
                            _ => RESUME_BUTTON.to_owned(),
                        })
                        .text_size(px(font_size::CAPTION))
                        .test_id("update-resume-btn"),
                    )
                    .on_press(move |_: &()| store.modal_open.set(true)),
            );
        }
        body
    })
}
