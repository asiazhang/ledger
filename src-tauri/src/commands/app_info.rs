//! 应用构建元信息命令面（issue #1957 / ADR-0022 修订注记）：设置页「关于」
//! Tab 与更新卡片的版本身份单一取数口。壳层机制——不进基础设施、不立业务域
//! crate 与壳内域目录（ADR-0111 归位判据）。
//!
//! 三个字段各自的可信来源：版本取编译期 `CARGO_PKG_VERSION`（根包版本，与
//! `package.json` / `tauri.conf.json` 由守门 `scripts/check-versions.ts` 钉成
//! 同一个数）；平台与架构取编译期 `std::env::consts`——**不做 UA 推断**：
//! WebView 的 UA 在 Apple Silicon 上仍报 Intel，"名实不符"正是本票要修的缺陷。

use serde::Serialize;

/// 构建元信息（IPC 载荷）：应用版本 + 编译期平台与架构。
///
/// 平台与架构保留 `std::env::consts` 的原值（`macos` / `aarch64` 形态），显示名
/// 映射归前端（`src/settings/useAppBuildInfo.ts`）：本层不做展示口径判断。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppBuildInfo {
    /// 应用版本（根包 `Cargo.toml`，与 `package.json` / `tauri.conf.json` 全等）。
    pub version: String,
    /// 编译期平台：`macos` / `windows` / `linux` / `android` / `ios`。
    pub platform: String,
    /// 编译期架构：`aarch64` / `x86_64` / `arm` 等。
    pub arch: String,
}

/// 报告应用构建元信息（设置页「关于」Tab 与更新卡片的当前版本号共用）。
///
/// 无参数、无账本接触、恒成功——失败面只有序列化（不会发生），故不返回
/// `Result`，前端也就不必为它准备错误文案。
#[tauri::command]
pub fn app_build_info() -> AppBuildInfo {
    AppBuildInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}
