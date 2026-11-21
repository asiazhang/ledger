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

use tauri_app_lib::commands::app_info::app_build_info;

const TAURI_CONFIG: &str = include_str!("../../tauri.conf.json");

#[test]
fn app_build_info_version_is_the_packaged_version() {
    let info = app_build_info();
    let config: serde_json::Value =
        serde_json::from_str(TAURI_CONFIG).expect("tauri.conf.json 应可解析");
    let packaged = config["version"]
        .as_str()
        .expect("tauri.conf.json 应声明 version");

    assert_eq!(info.version, packaged);
}

#[test]
fn app_build_info_platform_and_arch_are_compile_time_consts() {
    let info = app_build_info();

    assert_eq!(info.platform, std::env::consts::OS);
    assert_eq!(info.arch, std::env::consts::ARCH);
    assert!(!info.platform.is_empty() && !info.arch.is_empty());
}
