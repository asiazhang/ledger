//! 连接层统一 DB 调用 helper `db::run_db` 的纯单测（形状乙，spec #498 / #501）：
//! 闭包在 tauri 全局运行时的阻塞线程池执行（不在调用线程内联）、业务 `Result`
//! 原样传播（Ok 值 / Err 不二次包装）、闭包 panic（JoinError）归一化为
//! [`AppError::Io`]、调用方 tracing 上下文（dispatcher + 当前 span）跨线程
//! 传播（#503：HTTP handlers 迁入 helper 后 SQL 归因不得漂移）。

use std::sync::{Arc, Mutex};

use crate::error::AppError;
use crate::test_utils::{CaptureLayer, ensure_global_max_level};

use tracing_subscriber::layer::SubscriberExt;

use super::super::run_db;

/// 闭包执行：闭包在阻塞线程池线程跑完，Ok 值原样带回 await 点。
#[test]
fn closure_runs_off_caller_thread_and_returns_value() {
    let caller = std::thread::current().id();
    let result = tauri::async_runtime::block_on(run_db("test", move || {
        assert_ne!(
            std::thread::current().id(),
            caller,
            "闭包应在阻塞线程池线程执行，不在调用线程内联"
        );
        Ok(41 + 1)
    }))
    .expect("helper 应传播闭包的 Ok 值");
    assert_eq!(result, 42);
}

/// Result 传播（Err 路径）：闭包返回的业务错误原样透传，不被 helper 二次包装。
#[test]
fn closure_err_propagates_verbatim() {
    let err = tauri::async_runtime::block_on(super::super::run_db::<(), _>("test", || {
        Err(AppError::Invalid("boom".into()))
    }))
    .unwrap_err();
    assert!(
        matches!(err, AppError::Invalid(ref m) if m == "boom"),
        "业务错误应原样传播，实际 {err:?}"
    );
}

/// 调用方上下文传播（#503）：调用点已有活动 span + 线程局部 dispatcher
///（HTTP handlers 先例：tower_http 请求 span「request」）时，闭包在该 span 内
/// 执行、事件路由到调用方 dispatcher——线程局部上下文不会自动跨线程，靠 helper
/// 显式带入，SQL 耗时归因因此不因迁移漂移（API 集成测试契约
/// `test_http_sql_duration_attributed_to_request_span` 钉死的行为）。
#[test]
fn caller_span_and_dispatch_propagate_into_closure() {
    ensure_global_max_level();
    let layer = CaptureLayer::new();
    let captured = Arc::clone(&layer.events);
    let subscriber = tracing_subscriber::registry().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    let observed = Arc::new(Mutex::new(None::<String>));
    let obs = observed.clone();
    let span = tracing::info_span!("request");
    let _entered = span.enter();
    tauri::async_runtime::block_on(run_db("test", move || {
        tracing::info!("闭包内标记事件");
        *obs.lock().unwrap() = tracing::Span::current()
            .metadata()
            .map(|m| m.name().to_string());
        Ok(())
    }))
    .expect("helper 应传播闭包的 Ok 值");

    assert_eq!(
        observed.lock().unwrap().as_deref(),
        Some("request"),
        "闭包应携带调用方 span 执行（跨线程显式带入）"
    );
    let events = captured.lock().unwrap().clone();
    let marker: Vec<_> = events
        .iter()
        .filter(|e| e.fields.iter().any(|(k, _)| k == "message"))
        .collect();
    assert!(
        !marker.is_empty(),
        "闭包内事件应路由到调用方 dispatcher（否则线程局部捕获收不到），实际: {events:?}"
    );
    assert!(
        marker
            .iter()
            .all(|e| e.current_span.as_deref() == Some("request")),
        "闭包内事件应归因到调用方 span（request），实际: {events:?}"
    );
}

/// 无调用方 span（IPC 异步命令先例）时重建 `command` span 兜底，维持既有
/// SQL 耗时归因（lib.rs 异步命令归因约定，#501/#502 全部命令依赖的行为）。
#[test]
fn command_span_rebuilt_when_caller_has_none() {
    // 全局 no-op subscriber：无 subscriber 时 span 一律 disabled（旧实现同样如此），
    // 断言重建行为需先保证 span 可启用；阻塞线程池经捕获的 dispatcher 继承。
    ensure_global_max_level();
    let observed = Arc::new(Mutex::new(None::<String>));
    let obs = observed.clone();
    tauri::async_runtime::block_on(run_db("test", move || {
        *obs.lock().unwrap() = tracing::Span::current()
            .metadata()
            .map(|m| m.name().to_string());
        Ok(())
    }))
    .expect("helper 应传播闭包的 Ok 值");
    assert_eq!(
        observed.lock().unwrap().as_deref(),
        Some("command"),
        "调用点无 span 时应重建 command span（lib.rs 归因约定兜底）"
    );
}

/// 闭包 panic → JoinError 归一化为 [`AppError::Io`]（helper 的错误归一化路径，
/// 与 `spawn_blocking` 先例 `fetch_fund_detail_for_api` 同形）。
#[test]
fn closure_panic_maps_to_io_error() {
    let err = tauri::async_runtime::block_on(super::super::run_db::<(), _>(
        "test",
        || -> crate::error::Result<()> {
            panic!("闭包内崩溃");
        },
    ))
    .unwrap_err();
    assert!(
        matches!(err, AppError::Io(_)),
        "panic 应映射为 AppError::Io，实际 {err:?}"
    );
}
