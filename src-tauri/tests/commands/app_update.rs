//! 应用更新检查与安装命令面集成测试（issue #1429 / #1430 / ADR-0124 决策 4/6）。
//!
//! 直调命令函数（`#[tokio::test]`），覆盖壳行为：版本比较三态（更新/相同/更旧）、
//! 更新清单异常路径码化错误（更新源不可达、清单畸形）、自动检查开关对启动延迟
//! 静默检查的消费（关 → 不发请求；开启 → 发请求并通知前端）；安装路径编排
//! （issue #1430 / #1944）：下载零备份动作（下载 = 取更新对象 + 带进度下载 +
//! 暂存，进度事件可观察）、安装确认后先更新前备份后安装（备份失败即中止安装 +
//! 码化错误，暂存工件保留、重试零重新下载；删除接线即红的负向条目以空暂存 +
//! 坏备份目录下报错码的先后承载）、覆盖式保留最近一份、验签失败拒绝、无更新
//! 码化错误、静默检查零下载。测试边界（ADR-0124 测试决策）：任何用例都不得
//! 触达真实 `update.install`（各平台安装器动作归手动冒烟）——安装编排用例要么
//! 暂存为空（在备份或取件处中止），要么止于备份失败。更新源不打桩库：
//! 经插件端点注入（`check_update` / `run_silent_check` / `download_update` 的端点
//! 覆盖接缝，生产传 `None` 走应用配置）指向测试内起的本地真 HTTP 服务
//! （127.0.0.1:0，进程内起服务先例同 api_server / s3 套件）。命令级错误路径直接
//! 走夹具配置的更新源端点（127.0.0.1:1，确定拒绝）。工件下载与验签走真实插件
//! 链路（夹具 minisign 密钥对，见 [`FIXTURE_ARTIFACT_SIGNATURE`] 注）；二进制
//! 替换（各平台安装器）不测（上游职责 + 真实三平台链路归手动冒烟，
//! ADR-0124 测试决策）。
//!
//! 应用夹具：updater 插件消费 `plugins.updater` 配置（pubkey 必填，缺失即插件
//! 初始化失败），而 `tauri::Context` 的 config 字段私有、mock_context 无法注入
//! ——故经 `generate_context!` 编译期装入夹具配置
//! （`fixtures/updater/tauri.conf.json`，版本 0.1.0），生成的 Context 为
//! 泛型，与 MockRuntime 组合。插件句柄由此在集成测试内可得（ADR-0124 测试决策
//! 的「插件句柄不可得」回退分支不触发）。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ledger_backup::{BackupKind, PRE_UPDATE_BACKUP_FILE_NAME, read_backup_kind};
use ledger_infra::db::DbState;
use ledger_infra::settings::{SettingKey, get as get_setting};
use tauri::Listener;
use tauri::Manager;
use tauri_app_lib::app_update::UpdateSurface;
use tauri_app_lib::app_update::{
    AutoUpdateSwitch, DownloadedUpdate, PreUpdateBackupDirSlot, UPDATE_AVAILABLE_EVENT,
    UPDATE_DOWNLOAD_PROGRESS_EVENT, UpdateAvailablePayload, UpdateCheckOutcome, UpdateCheckStatus,
    UpdateDownloadOutcome, UpdateDownloadProgress, check_update, download_update, run_silent_check,
};
use tauri_app_lib::commands::app_update::{
    check_for_updates, download_update as download_update_command, get_update_surface,
    install_update, set_auto_update_enabled,
};

/// 夹具配置的更新源端点（127.0.0.1:1，tcpmux 特权端口，无进程监听，连接确定
/// 被拒——不经 bind-then-drop：释放端口可能被并行测试的清单服务复用）。
const UNREACHABLE_ENDPOINT: &str = "http://127.0.0.1:1/latest.json";

/// 更新源清单夹具（生产形态契约 = scripts/generate-update-manifest.ts 头注：
/// version 三段式语义版本、notes 发布说明、pub_date RFC3339、platforms 三平台
/// 闭集各携 url+signature）。
fn manifest_json(version: &str, notes: &str) -> String {
    format!(
        r#"{{
  "version": "{version}",
  "notes": "{notes}",
  "pub_date": "2026-10-11T00:00:00Z",
  "platforms": {{
    "darwin-aarch64": {{ "url": "https://releases.example.invalid/OpenLedger.app.tar.gz", "signature": "dGVzdA==" }},
    "windows-x86_64": {{ "url": "https://releases.example.invalid/OpenLedger.exe", "signature": "dGVzdA==" }},
    "linux-x86_64": {{ "url": "https://releases.example.invalid/OpenLedger.AppImage", "signature": "dGVzdA==" }}
  }}
}}"#
    )
}

/// 测试内起的本地真 HTTP 源：恒定应答固定响应体并计数请求数（清单源与
/// 工件源同构，仅 Content-Type / 响应体 / 路径不同）。手写极简 HTTP/1.1
/// 应答（一请求一连接、应答即关），不引入桩库。请求数 = 该动作发生了几次，
/// 编排断言的核心观察点。
struct HttpStubSource {
    /// 端点 URL（注入检查接缝 endpoint_override 或清单 url 字段的值）
    endpoint: String,
    requests: Arc<AtomicUsize>,
}

/// 启动 HTTP 桩服务（每测试独立端口，互不共享）。
fn spawn_http_stub(
    body: Vec<u8>,
    content_type: &'static str,
    path: &'static str,
) -> HttpStubSource {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind 本地端口");
    let addr = listener.local_addr().expect("本地端口地址");
    let requests = Arc::new(AtomicUsize::new(0));
    let requests_thread = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            requests_thread.fetch_add(1, Ordering::SeqCst);
            // 读掉请求头（读到空行即可），应答固定响应体后关连接。
            let mut buf = [0u8; 4096];
            let mut seen = 0usize;
            loop {
                match std::io::Read::read(&mut stream, &mut buf[seen..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        seen += n;
                        if buf[..seen].windows(4).any(|w| w == b"\r\n\r\n") || seen == buf.len() {
                            break;
                        }
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            let _ = std::io::Write::write_all(&mut stream, &body);
            let _ = std::io::Write::flush(&mut stream);
        }
    });
    HttpStubSource {
        endpoint: format!("http://{addr}{path}"),
        requests,
    }
}

/// 启动清单服务（每测试独立端口，互不共享）。
fn spawn_update_source(body: String) -> HttpStubSource {
    spawn_http_stub(body.into_bytes(), "application/json", "/latest.json")
}

/// 端点覆盖接缝的参数值。
fn override_endpoint(endpoint: &str) -> Option<Vec<url::Url>> {
    Some(vec![url::Url::parse(endpoint).expect("端点 URL 合法")])
}

fn request_count(source: &HttpStubSource) -> usize {
    source.requests.load(Ordering::SeqCst)
}

/// 带 updater 插件与运行期开关镜像的 mock 应用：配置经
/// `fixtures/updater/tauri.conf.json` 装入（版本 0.1.0，插件 pubkey 与
/// 确定拒绝的更新源端点烧入 `plugins.updater`），开关镜像与生产 setup 同形登记。
fn updater_app() -> tauri::AppHandle<tauri::test::MockRuntime> {
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(tauri::generate_context!(
            "tests/commands/fixtures/updater/tauri.conf.json",
            test = true
        ))
        .expect("带 updater 插件的 mock 应用");
    // 运行期开关镜像（生产在 run() setup 登记，测试夹具同形登记）。
    app.manage(AutoUpdateSwitch::default());
    app.handle().clone()
}

// ---------------------------------------------------------------------------
// 手动检查：版本比较三态 + 异常路径码化错误
// ---------------------------------------------------------------------------

#[tokio::test]
async fn manual_check_reports_newer_version_with_notes() {
    let source = spawn_update_source(manifest_json("1.2.3", "新版本说明，含兼容性警告"));
    let app = updater_app();

    let outcome = check_update(&app, override_endpoint(&source.endpoint))
        .await
        .expect("检查应成功");
    assert_eq!(outcome.status, UpdateCheckStatus::Update);
    assert_eq!(outcome.version.as_deref(), Some("1.2.3"));
    assert_eq!(outcome.notes.as_deref(), Some("新版本说明，含兼容性警告"));
    assert_eq!(request_count(&source), 1);
}

#[tokio::test]
async fn manual_check_reports_same_version() {
    // 夹具应用包版本 = 0.1.0：清单同版本 → same，不携版本与说明。
    let source = spawn_update_source(manifest_json("0.1.0", "同版本说明"));
    let app = updater_app();

    let outcome = check_update(&app, override_endpoint(&source.endpoint))
        .await
        .expect("检查应成功");
    assert_eq!(outcome.status, UpdateCheckStatus::Same);
    assert_eq!(outcome.version, None);
    assert_eq!(outcome.notes, None);
}

#[tokio::test]
async fn manual_check_reports_older_version() {
    let source = spawn_update_source(manifest_json("0.0.9", "旧版本说明"));
    let app = updater_app();

    let outcome = check_update(&app, override_endpoint(&source.endpoint))
        .await
        .expect("检查应成功");
    assert_eq!(outcome.status, UpdateCheckStatus::Older);
    assert_eq!(outcome.version, None);
    assert_eq!(outcome.notes, None);
}

#[tokio::test]
async fn manual_check_unreachable_source_is_coded_error() {
    let app = updater_app();

    let err = check_update(&app, override_endpoint(UNREACHABLE_ENDPOINT))
        .await
        .expect_err("不可达应报错");
    assert_eq!(err.code(), Some("update.unreachable"));
}

#[tokio::test]
async fn manual_check_malformed_manifest_is_coded_error() {
    // 畸形形态一：缺 version 字段的清单体（反序列化失败）。
    let source = spawn_update_source("{\"notes\": \"缺版本\"}".to_string());
    let app = updater_app();
    let err = check_update(&app, override_endpoint(&source.endpoint))
        .await
        .expect_err("畸形清单应报错");
    assert_eq!(err.code(), Some("update.manifest-invalid"));
}

#[tokio::test]
async fn manual_check_manifest_missing_platform_entry_is_coded_error() {
    // 畸形形态二：platforms 缺当前平台条目（TargetsNotFound）。
    let source = spawn_update_source(
        r#"{
  "version": "1.2.3",
  "notes": "无平台条目",
  "pub_date": "2026-10-11T00:00:00Z",
  "platforms": {}
}"#
        .to_string(),
    );
    let app = updater_app();
    let err = check_update(&app, override_endpoint(&source.endpoint))
        .await
        .expect_err("缺平台条目应报错");
    assert_eq!(err.code(), Some("update.manifest-invalid"));
}

// ---------------------------------------------------------------------------
// 命令壳：手动检查命令走应用配置端点（夹具 = 确定拒绝）+ 开关推送命令
// ---------------------------------------------------------------------------

#[tokio::test]
async fn check_for_updates_command_surfaces_coded_error_from_config_endpoint() {
    // 命令传 None（生产路径）：端点来自应用配置（夹具烧入 127.0.0.1:1）——
    // 壳层命令面 → 唯一调用点 → 应用配置端点的整链错误路径。
    let app = updater_app();

    let err = check_for_updates(app)
        .await
        .expect_err("配置端点被拒应报错");
    assert_eq!(err.code(), Some("update.unreachable"));
}

#[tokio::test]
async fn set_auto_update_enabled_pushes_runtime_mirror() {
    let app = updater_app();

    set_auto_update_enabled(app.clone(), false).expect("推送开关应成功");
    // 镜像生效由静默检查行为观察（下一用例），此处只钉命令成功面。
    set_auto_update_enabled(app.clone(), true).expect("推送开关应成功");
}

// ---------------------------------------------------------------------------
// 自动检查开关（运行期镜像）+ 启动延迟静默检查的单轮执行体
// ---------------------------------------------------------------------------

#[tokio::test]
async fn auto_check_skips_request_when_switch_off() {
    let source = spawn_update_source(manifest_json("1.2.3", "不应被请求"));
    let app = updater_app();

    set_auto_update_enabled(app.clone(), false).expect("推送开关");
    let outcome = run_silent_check(&app, override_endpoint(&source.endpoint)).await;

    assert_eq!(outcome, None);
    assert_eq!(request_count(&source), 0, "开关关闭时不得发起检查请求");
}

#[tokio::test]
async fn auto_check_runs_when_switch_on_and_emits_update_available() {
    let source = spawn_update_source(manifest_json("1.2.3", "自动检查发现的版本"));
    let app = updater_app();

    set_auto_update_enabled(app.clone(), true).expect("推送开关");

    let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let seen_listener = Arc::clone(&seen);
    app.listen(UPDATE_AVAILABLE_EVENT, move |event| {
        *seen_listener.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(event.payload().to_string());
    });

    let outcome = run_silent_check(&app, override_endpoint(&source.endpoint)).await;

    assert_eq!(request_count(&source), 1, "开关开启时应发起检查");
    let outcome: UpdateCheckOutcome = outcome.expect("检查应有结果");
    assert_eq!(outcome.status, UpdateCheckStatus::Update);

    // 发现新版本 → 前端通知事件携版本号与发布说明（mock runtime 未运行时
    // 主线程投递内联执行，await 返回后事件已送达）。
    let payload = seen.lock().unwrap_or_else(|e| e.into_inner()).take();
    let payload: UpdateAvailablePayload =
        serde_json::from_str(payload.as_deref().expect("应发出更新事件")).expect("事件载荷合法");
    assert_eq!(payload.version, "1.2.3");
    assert_eq!(payload.notes.as_deref(), Some("自动检查发现的版本"));
}

#[tokio::test]
async fn auto_check_is_silent_on_failure() {
    let app = updater_app();
    set_auto_update_enabled(app.clone(), true).expect("推送开关");

    let outcome = run_silent_check(&app, override_endpoint(UNREACHABLE_ENDPOINT)).await;

    // 后台检查失败完全静默：返回空、不 panic、不打扰（ADR-0124 决策 4）。
    assert_eq!(outcome, None);
}

#[tokio::test]
async fn auto_check_defaults_to_enabled_on_fresh_app() {
    // 新装默认开启（ADR-0124 决策 8）：未经推送的全新应用镜像为开启——
    // 开关是应用级状态（每测试夹具新应用即全新镜像），默认值由此直接可观察。
    let source = spawn_update_source(manifest_json("1.2.3", "默认开启即检查"));
    let app = updater_app();

    let outcome = run_silent_check(&app, override_endpoint(&source.endpoint)).await;

    assert_eq!(
        request_count(&source),
        1,
        "缺省（未推送）应按开启兜底发起检查"
    );
    assert_eq!(outcome.map(|o| o.status), Some(UpdateCheckStatus::Update));
}

// ---------------------------------------------------------------------------
// 安装路径（issue #1430 / ADR-0124 决策 6）：更新前备份编排 + 询问式下载安装
// ---------------------------------------------------------------------------

/// 夹具更新工件字节（内容任意：编排测试只消费「下载发生了、字节到手、验签通过」，
/// 不消费工件内容——平台安装器差异不在本票测试面）。
const FIXTURE_ARTIFACT: &[u8] = b"fixture-update-artifact-bytes-v1";

/// 夹具工件签名（.sig 文件内容，base64 文本原样内嵌清单 signature 字段）。
/// 由测试专用 minisign 密钥对生成（私钥不入库，密钥泄漏无生产影响——夹具公钥
/// 只烧入本测试夹具配置，与生产公钥无关）；对 [`FIXTURE_ARTIFACT`] 签发，
/// trusted comment 绑定版本 1.2.3。重新生成：
/// `pnpm exec tauri signer generate -w <key> --password "" --ci` 后
/// `pnpm exec tauri signer sign -f <key> -p "" --app-version 1.2.3 <artifact>`，
/// 公钥同步烧入 `fixtures/updater/tauri.conf.json`。
const FIXTURE_ARTIFACT_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSS1R6bStUNXR6YjBqR2puUzdmU2gralhlZW4zR0tzbzMzM0lsU0RuZDI5cHFDK0FmblVaZlkySmhLYXkrUjRWSHNqSDdGV2FXZHRYNjZtWmFmVFRNNFVQejl1VHd6WWdzPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxOTAxNjUyCWZpbGU6YXJ0aWZhY3QuYmluCXZlcnNpb246MS4yLjMKTE56VzhucFBxZmszYWZwN3Ava0h2eklDcE04N1lnSTBmamNlQ2hTNGxNeTd2K3BFOUJHaWJxckpSeEF4L3RkU1Rwd3JsOEVWelpSMnBTbkpadnZ1Q3c9PQo=";

/// 当前测试平台的 updater target 键（与插件 updater_os + arch 同规则，
/// generate-update-manifest.ts 的平台键闭集）。
fn current_updater_target() -> &'static str {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "darwin-aarch64"
        } else {
            "darwin-x86_64"
        }
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}

/// 启动工件源（每测试独立端口）：恒定应答夹具工件字节；请求数 = 下载动作
/// 发生了几次，编排断言的核心观察点（计数经 [`request_count`]）。
fn spawn_artifact_source(body: &'static [u8]) -> HttpStubSource {
    spawn_http_stub(body.to_vec(), "application/octet-stream", "/artifact.bin")
}

/// 下载编排用清单：三平台条目全部指向本地工件源并携带夹具签名（检查只读
/// 当前平台条目，其余平台条目同形不影响断言）。
fn download_manifest_json(version: &str, notes: &str, artifact_url: &str) -> String {
    let entry =
        format!(r#"{{ "url": "{artifact_url}", "signature": "{FIXTURE_ARTIFACT_SIGNATURE}" }}"#);
    format!(
        r#"{{
  "version": "{version}",
  "notes": "{notes}",
  "pub_date": "2026-10-11T00:00:00Z",
  "platforms": {{
    "darwin-aarch64": {entry},
    "windows-x86_64": {entry},
    "linux-x86_64": {entry}
  }}
}}"#
    )
}

/// 更新前备份目录换装的临时目录（RAII，drop 整棵删除）。
fn pre_update_scratch() -> tauri_app_lib::test_support::ScratchDir {
    tauri_app_lib::test_support::ScratchDir::new("updatetest-preupdate")
}

/// 安装路径运行态装配（内存库 + 下载暂存槽 + 更新前备份目录换装），夹具间
/// 共享：建库经 test_support 工厂（ADR-0084 建库唯一入口）；内存库写读两槽
/// 同指一连接（测试世界形态，ADR-0125 决策 4）。
fn manage_install_state(
    app: &tauri::AppHandle<tauri::test::MockRuntime>,
    backup_dir: std::path::PathBuf,
) {
    let shared = Arc::new(Mutex::new(tauri_app_lib::test_support::open()));
    app.manage(DbState {
        conn: Arc::clone(&shared),
        read_conn: shared,
    });
    app.manage(DownloadedUpdate::default());
    app.manage(PreUpdateBackupDirSlot(Some(backup_dir)));
}

/// 带安装路径完整状态的 mock 应用：updater 插件（夹具配置）+ 内存库 +
/// 下载暂存槽 + 更新前备份目录换装（健康临时目录，不触真实用户数据目录）。
/// 下载编排测试也复用本夹具——暂存槽是下载的产物落点，健康备份目录用于
/// 反证下载零备份动作（#1944）。
fn install_app() -> (
    tauri::AppHandle<tauri::test::MockRuntime>,
    tauri_app_lib::test_support::ScratchDir,
) {
    let app = updater_app();
    let scratch = pre_update_scratch();
    manage_install_state(&app, scratch.path().to_path_buf());
    (app, scratch)
}

/// 备份必然失败的安装夹具：备份目录指向不存在的子目录（坏路径注入，
/// SyncChannelsSlot 同款测试换装）。内存库与暂存槽同 [`install_app`]。
fn install_app_with_dead_backup_dir() -> (
    tauri::AppHandle<tauri::test::MockRuntime>,
    tauri_app_lib::test_support::ScratchDir,
) {
    let app = updater_app();
    let scratch = pre_update_scratch();
    manage_install_state(&app, scratch.join("no-such-dir"));
    (app, scratch)
}

fn backup_file(scratch: &std::path::Path) -> std::path::PathBuf {
    scratch.join(PRE_UPDATE_BACKUP_FILE_NAME)
}

#[tokio::test]
async fn download_update_downloads_and_stages_without_backup() {
    let artifact = spawn_artifact_source(FIXTURE_ARTIFACT);
    let source = spawn_update_source(download_manifest_json(
        "1.2.3",
        "安装路径夹具版本",
        &artifact.endpoint,
    ));
    let (app, scratch) = install_app();

    let seen: Arc<Mutex<Vec<UpdateDownloadProgress>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_listener = Arc::clone(&seen);
    app.listen(UPDATE_DOWNLOAD_PROGRESS_EVENT, move |event| {
        let payload: UpdateDownloadProgress =
            serde_json::from_str(event.payload()).expect("进度事件载荷合法");
        seen_listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(payload);
    });

    let outcome = download_update(&app, override_endpoint(&source.endpoint))
        .await
        .expect("下载编排应成功");

    assert_eq!(
        outcome,
        UpdateDownloadOutcome {
            version: "1.2.3".to_string()
        }
    );
    // 下载不含备份动作（备份移至安装确认后，#1944）：即便备份目录健康、内存库
    // 在位，下载编排也不产生任何更新前备份产物。
    let entries: Vec<_> = std::fs::read_dir(&scratch)
        .expect("备份目录可读")
        .collect::<Result<_, _>>()
        .expect("目录项可读");
    assert!(
        entries.is_empty(),
        "下载编排不得创建更新前备份（备份时机在安装确认后）"
    );
    // 工件下载恰好发生一次，且来自下载确认（不是检查）。
    assert_eq!(request_count(&artifact), 1);
    // 进度对前端可观察：至少收尾一次，最终累计 = 工件总大小。
    let events = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(!events.is_empty(), "下载进度事件应至少发射一次（收尾必发）");
    assert_eq!(
        events.last().unwrap().received,
        FIXTURE_ARTIFACT.len() as u64
    );
    // 工件已暂存待装：二次确认安装的入口就绪。
    assert!(app.state::<DownloadedUpdate>().is_pending());
}

#[tokio::test]
async fn download_update_rejects_artifact_with_invalid_signature() {
    let artifact = spawn_artifact_source(FIXTURE_ARTIFACT);
    // 签名与夹具公钥不配对：清单条目携带占位签名（与检查路径畸形签名同款）。
    let body = format!(
        r#"{{
  "version": "1.2.3",
  "notes": "坏签名",
  "pub_date": "2026-10-11T00:00:00Z",
  "platforms": {{
    "{}": {{ "url": "{}", "signature": "dGVzdA==" }}
  }}
}}"#,
        current_updater_target(),
        artifact.endpoint
    );
    let source = spawn_update_source(body);
    let (app, _scratch) = install_app();

    let err = download_update(&app, override_endpoint(&source.endpoint))
        .await
        .expect_err("验签失败应报错");

    assert_eq!(err.code(), Some("update.signature-invalid"));
    assert_eq!(request_count(&artifact), 1, "下载发生了才走到验签");
    assert!(
        !app.state::<DownloadedUpdate>().is_pending(),
        "坏工件不暂存"
    );
}

#[tokio::test]
async fn download_update_without_newer_version_is_coded_error() {
    let artifact = spawn_artifact_source(FIXTURE_ARTIFACT);
    // 夹具应用包版本 = 0.1.0：清单同版本 → 无更新可装。
    let source = spawn_update_source(download_manifest_json(
        "0.1.0",
        "同版本",
        &artifact.endpoint,
    ));
    let (app, _scratch) = install_app();

    let err = download_update(&app, override_endpoint(&source.endpoint))
        .await
        .expect_err("无更新应报错");

    assert_eq!(err.code(), Some("update.no-update"));
    assert_eq!(request_count(&artifact), 0);
}

#[tokio::test]
async fn install_update_backs_up_before_reporting_missing_download() {
    let (app, scratch) = install_app();

    let err = install_update(app.clone())
        .await
        .expect_err("未下载即安装应报错");
    assert_eq!(err.code(), Some("update.not-downloaded"));

    // 先备份、后安装（#1944 / ADR-0124 决策 6）：即便是注定失败的安装尝试
    // （暂存槽为空），更新前备份也已先于取件完成——编排次序正一步可观察。
    assert!(
        backup_file(&scratch).exists(),
        "安装确认后应先创建更新前备份"
    );
    assert_eq!(
        read_backup_kind(&backup_file(&scratch)).unwrap(),
        BackupKind::PreUpdate
    );
    // 不标脏：更新前备份是裸作业，不经统一写入口（DirtyMarker 语义，ADR-0032）。
    let read_conn = app.state::<DbState>().read_conn.clone();
    let dirty = get_setting(
        &read_conn.lock().unwrap_or_else(|e| e.into_inner()),
        SettingKey::AutoBackupDirty,
        false,
    )
    .expect("读取脏标记设置");
    assert!(!dirty);
}

#[tokio::test]
async fn install_update_backup_failure_aborts_before_install() {
    // 删除接线即红（#1944）：备份接线删除或移到取件之后，本用例的报错码都会
    // 退回 `update.not-downloaded`（暂存槽为空，不会触达真实安装器）。
    let (app, _scratch) = install_app_with_dead_backup_dir();

    let err = install_update(app).await.expect_err("备份失败应中止安装");
    assert_eq!(
        err.code(),
        Some("update.backup-failed"),
        "备份失败必须先于取件与安装被观察到"
    );
}

#[tokio::test]
async fn install_update_backup_failure_keeps_staged_artifact_for_retry() {
    let artifact = spawn_artifact_source(FIXTURE_ARTIFACT);
    let source = spawn_update_source(download_manifest_json(
        "1.2.3",
        "备份失败重试夹具",
        &artifact.endpoint,
    ));
    // 备份目录指向坏路径：下载不再依赖备份目录（#1944），可先正常下载暂存，
    // 再让安装编排的备份步必然失败。
    let (app, _scratch) = install_app_with_dead_backup_dir();

    download_update(&app, override_endpoint(&source.endpoint))
        .await
        .expect("下载应成功（下载编排零备份动作）");
    assert!(app.state::<DownloadedUpdate>().is_pending());

    let err = install_update(app.clone())
        .await
        .expect_err("备份失败应中止安装");
    assert_eq!(err.code(), Some("update.backup-failed"));
    // 暂存工件保留：备份失败不消费工件，现场可重试。
    assert!(
        app.state::<DownloadedUpdate>().is_pending(),
        "备份失败后已下载工件必须留在暂存"
    );

    // 重试无需重新下载：再次确认安装（备份仍失败）不触发任何新的工件请求。
    let err = install_update(app)
        .await
        .expect_err("重试同样因备份失败中止");
    assert_eq!(err.code(), Some("update.backup-failed"));
    assert_eq!(request_count(&artifact), 1, "安装重试不得重新下载工件");
}

#[tokio::test]
async fn install_update_overwrites_previous_backup() {
    let (app, scratch) = install_app();

    install_update(app.clone())
        .await
        .expect_err("第一次安装尝试（暂存为空）应报错");
    install_update(app)
        .await
        .expect_err("第二次安装尝试（暂存为空）应报错");

    // 覆盖式保留最近一份：固定名产物覆盖上一份，不堆积。
    let entries: Vec<_> = std::fs::read_dir(&scratch)
        .expect("备份目录可读")
        .collect::<Result<_, _>>()
        .expect("目录项可读");
    assert_eq!(entries.len(), 1, "目录内只有最近一份更新前备份");
    assert_eq!(
        read_backup_kind(&backup_file(&scratch)).unwrap(),
        BackupKind::PreUpdate
    );
}

#[tokio::test]
async fn silent_check_never_downloads_artifact() {
    // 无静默路径：启动延迟静默检查只查清单，发现新版本也不下载工件
    // （下载只发生在用户确认后的 download_update）。
    let artifact = spawn_artifact_source(FIXTURE_ARTIFACT);
    let source = spawn_update_source(download_manifest_json(
        "1.2.3",
        "静默检查不下载",
        &artifact.endpoint,
    ));
    let app = updater_app();
    set_auto_update_enabled(app.clone(), true).expect("推送开关");

    let outcome = run_silent_check(&app, override_endpoint(&source.endpoint)).await;

    assert_eq!(outcome.map(|o| o.status), Some(UpdateCheckStatus::Update));
    assert_eq!(request_count(&artifact), 0, "静默检查不得下载工件");
}

#[tokio::test]
async fn download_update_command_surfaces_error_from_config_endpoint() {
    // 命令壳直连应用配置端点（夹具 = 确定拒绝）：壳三件套——命令面、错误码
    // 原样上抛；下载编排零备份动作（备份时机在安装确认后，#1944）。
    let (app, scratch) = install_app();

    let err = download_update_command(app)
        .await
        .expect_err("配置端点被拒应报错");
    assert_eq!(err.code(), Some("update.unreachable"));
    let entries: Vec<_> = std::fs::read_dir(&scratch)
        .expect("备份目录可读")
        .collect::<Result<_, _>>()
        .expect("目录项可读");
    assert!(entries.is_empty(), "下载编排不得创建更新前备份");
}

#[test]
fn get_update_surface_reports_runtime_platform() {
    // 平台分支信号（issue #1431）：命令返回值落在形态闭集内，且与运行平台一致——
    // macOS / Windows 恒 updater；移动端恒 release-page；Linux 按进程 `APPIMAGE`
    // 观测分辨 AppImage（updater）与 deb（release-page）。测试进程不以 AppImage
    // 运行（CI 与开发机均为裸二进制），Linux 分支按当前环境如实断言两侧。
    let surface = get_update_surface().expect("形态查询不失败");
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        assert_eq!(surface, UpdateSurface::Updater);
    } else if cfg!(target_os = "android") || cfg!(target_os = "ios") {
        assert_eq!(surface, UpdateSurface::ReleasePage);
    } else if cfg!(target_os = "linux") {
        let expected = if std::env::var_os("APPIMAGE").is_some() {
            UpdateSurface::Updater
        } else {
            UpdateSurface::ReleasePage
        };
        assert_eq!(surface, expected);
    }
}
