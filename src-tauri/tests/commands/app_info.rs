//! 构建元信息命令面集成测试（issue #1957 / ADR-0022 修订注记）：钉住「关于」
//! Tab 的版本身份——命令上报的版本必须是**打包进安装包的那个版本**
//! （`tauri.conf.json` 的 `version`），平台与架构必须是编译期常量。
//!
//! 这是本票缺陷的回归位：改前「关于」页读前端 `package.json` 的 `version`，
//! 与安装包/更新比较所用的 `tauri.conf.json` 版本是两处独立双写；改后单一
//! 取数口 = 本命令，故命令值与应用配置的相等关系就是用户可观察口径的契约。
//! 三方文件（`package.json` / `tauri.conf.json` / 根 `Cargo.toml`）的全等归
//! 守门 `scripts/check-versions.ts`（构建期），本文件只核命令一侧。

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

use tauri_app_lib::commands::app_info::app_build_info;

/// 应用配置（编译期装入）：版本真源的对照面。
const TAURI_CONFIG: &str = include_str!("../../tauri.conf.json");

#[test]
fn app_build_info_version_is_the_packaged_version() {
    let info = app_build_info();
    let config: serde_json::Value =
        serde_json::from_str(TAURI_CONFIG).expect("tauri.conf.json 应可解析");
    let packaged = config["version"]
        .as_str()
        .expect("tauri.conf.json 应声明 version");

    // 用户可观察口径：设置页显示的版本 == 安装包版本（更新比较用的同一个数）
    assert_eq!(info.version, packaged);
}

#[test]
fn app_build_info_platform_and_arch_are_compile_time_consts() {
    let info = app_build_info();

    // 编译期常量原值：不做 UA 推断（Apple Silicon 的 UA 仍报 Intel）
    assert_eq!(info.platform, std::env::consts::OS);
    assert_eq!(info.arch, std::env::consts::ARCH);
    assert!(!info.platform.is_empty() && !info.arch.is_empty());
}
