//! 应用更新检查命令面集成测试（issue #1429 / ADR-0124 决策 4）。
//!
//! 直调命令函数（`#[tokio::test]`），覆盖壳行为：版本比较三态（更新/相同/更旧）、
//! 更新清单异常路径码化错误（更新源不可达、清单畸形）、自动检查开关对启动延迟
//! 静默检查的消费（关 → 不发请求；开启 → 发请求并通知前端）。更新源不打桩库：
//! 经插件端点注入（`check_update` / `run_silent_check` 的端点覆盖接缝，生产传
//! `None` 走应用配置）指向测试内起的本地真 HTTP 服务（127.0.0.1:0，进程内起
//! 服务先例同 api_server / s3 套件）。命令级错误路径直接走夹具配置的更新源
//! 端点（127.0.0.1:1，确定拒绝）。插件自身的 HTTP 细节/验签/二进制替换不测
//! （上游职责，ADR-0124 测试决策）。
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

use tauri::Listener;
use tauri::Manager;
use tauri_app_lib::app_update::{
    AutoUpdateSwitch, UPDATE_AVAILABLE_EVENT, UpdateAvailablePayload, UpdateCheckOutcome,
    UpdateCheckStatus, check_update, run_silent_check,
};
use tauri_app_lib::commands::app_update::{check_for_updates, set_auto_update_enabled};

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

/// 测试内起的本地真 HTTP 更新源：恒定应答当前清单体并计数请求数。
/// 手写极简 HTTP/1.1 应答（一请求一连接、应答即关），不引入桩库。
struct UpdateSource {
    /// 端点 URL（注入检查接缝 endpoint_override 的值）
    endpoint: String,
    requests: Arc<AtomicUsize>,
}

/// 启动清单服务（每测试独立端口，互不共享）。
fn spawn_update_source(body: String) -> UpdateSource {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind 本地端口");
    let addr = listener.local_addr().expect("本地端口地址");
    let requests = Arc::new(AtomicUsize::new(0));
    let requests_thread = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            requests_thread.fetch_add(1, Ordering::SeqCst);
            // 读掉请求头（读到空行即可），应答固定清单体后关连接。
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
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            let _ = std::io::Write::flush(&mut stream);
        }
    });
    UpdateSource {
        endpoint: format!("http://{addr}/latest.json"),
        requests,
    }
}

/// 端点覆盖接缝的参数值。
fn override_endpoint(endpoint: &str) -> Option<Vec<url::Url>> {
    Some(vec![url::Url::parse(endpoint).expect("端点 URL 合法")])
}

fn request_count(source: &UpdateSource) -> usize {
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
