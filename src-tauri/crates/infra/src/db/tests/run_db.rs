use std::sync::{Arc, Mutex};

use crate::error::AppError;
use crate::test_utils::{CaptureLayer, ensure_global_max_level};

use tracing_subscriber::layer::SubscriberExt;

use super::super::run_db;

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

#[test]
fn command_span_rebuilt_when_caller_has_none() {
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
