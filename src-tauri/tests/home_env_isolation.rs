//! `test_support::isolate_home` 的契约测试（issue #1989）。
//!
//! 模拟「本机预置 `XDG_DATA_HOME` / `XDG_CONFIG_HOME`」的环境：现场隔离夹具必须
//! 在重定向 `$HOME` 的同时把两根 XDG 变量一并收进进程专属临时区——只重定向
//! `$HOME` 时 `dirs::data_dir()` 仍解析到真实 XDG 根，mock 应用的 `app_data_dir()`
//! 即对真实 `~/.local/share` 根做备份/恢复写入（#1989 事故）。
//!
//! 断言对准事故的同一条解析路径（mock 应用 `app_data_dir()` / `app_config_dir()`，
//! 均由 `dirs` 派生）；删除夹具内的 XDG 变量写入即红（删除即变红）。
//!
//! 进程级环境变量是全局量：本二进制只放这一个用例，毒化写入发生在夹具 `Once`
//! 之前，进程内无第二写入点、无并行竞态。已知边界：macOS 的 `dirs` 不消费
//! XDG 变量，该平台上本用例对「未重定向 XDG」的回归不敏感（事故仅Linux 形态）。

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

use tauri::Manager;
use tauri_app_lib::test_support::isolate_home;

/// 毒化 XDG 两根后调用隔离夹具：数据/配置目录解析必须落进隔离根，不得命中
/// 毒化路径（命中即说明夹具漏重定向对应变量）。
#[test]
fn isolate_home_redirects_xdg_data_and_config_roots() {
    let poison_data = std::env::temp_dir().join("ledger-test-home-poison-data");
    let poison_config = std::env::temp_dir().join("ledger-test-home-poison-config");
    // SAFETY：set_var 自 Rust 2024 起 unsafe；本写入先于夹具 Once（本进程唯一
    // 用例），毒化值只被夹具覆盖、不再被读取为真实现场。
    unsafe { std::env::set_var("XDG_DATA_HOME", &poison_data) };
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &poison_config) };

    isolate_home();

    let app = tauri::test::mock_app();
    let data_dir = app
        .path()
        .app_data_dir()
        .expect("mock app 应可解析 app_data_dir");
    let config_dir = app
        .path()
        .app_config_dir()
        .expect("mock app 应可解析 app_config_dir");

    assert_ne!(
        data_dir, poison_data,
        "app_data_dir 仍解析到毒化的 XDG_DATA_HOME：隔离夹具未重定向 XDG 变量（#1989 回潮）"
    );
    // mock 应用 identifier 为空，app_data_dir 与 XDG 根严格相等——本断言与后
    // 面的 starts_with 双保险中，本条是「XDG 根未重定向」的精确哨兵。
    assert_ne!(
        config_dir, poison_config,
        "app_config_dir 仍解析到毒化的 XDG_CONFIG_HOME：隔离夹具未重定向 XDG 变量（#1989 回潮）"
    );

    let home = std::env::var("HOME").expect("isolate_home 应已重定向 HOME");
    for dir in [data_dir, config_dir] {
        assert!(
            dir.starts_with(std::env::temp_dir()),
            "解析目录 {dir:?} 不在临时区：现场隔离失效"
        );
        assert!(
            dir.starts_with(&home),
            "解析目录 {dir:?} 不在隔离根 {home:?} 下：现场隔离失效"
        );
    }
}
