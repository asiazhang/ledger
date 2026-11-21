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

#[test]
fn isolate_home_redirects_xdg_data_and_config_roots() {
    let poison_data = std::env::temp_dir().join("ledger-test-home-poison-data");
    let poison_config = std::env::temp_dir().join("ledger-test-home-poison-config");
    // SAFETY：set_var 自 Rust 2024 起 unsafe；本写入先于夹具 Once（本进程唯一
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
