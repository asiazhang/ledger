//! 连接层统一 DB 调用 helper `db::run_db` 的纯单测（形状乙，spec #498 / #501）：
//! 闭包在 tauri 全局运行时的阻塞线程池执行（不在调用线程内联）、业务 `Result`
//! 原样传播（Ok 值 / Err 不二次包装）、闭包 panic（JoinError）归一化为
//! [`AppError::Io`]。

use crate::error::AppError;

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
