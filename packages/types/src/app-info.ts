// 应用构建元信息（issue #1957 / ADR-0022 修订注记）：设置页「关于」Tab 与更新
// 卡片的版本身份载荷类型。命令 `app_build_info` 的编译期真源与显示名映射分工：
// 平台与架构此处保留 `std::env::consts` 原值（`macos` / `aarch64`），显示名映射
// 归前端。

/** 应用构建元信息：应用版本 + 编译期平台与架构。 */
export interface AppBuildInfo {
  /** 应用版本（根包 `Cargo.toml`，与 `package.json` / `tauri.conf.json` 全等）。 */
  version: string;
  /** 编译期平台：`macos` / `windows` / `linux` / `android` / `ios`。 */
  platform: string;
  /** 编译期架构：`aarch64` / `x86_64` / `arm` 等。 */
  arch: string;
}
