# ADR-0074: Android 发布——GitHub Release 直装 APK、并入现有发布矩阵、iOS 暂缓

- 状态：已接受（实施待启动，grilling 定稿）
- 日期：2026-09-05
- 作者：Ledger 项目
- 关联：ADR-0066（发布矩阵与单一 Release 拓扑沿用；其「全平台暂不签名」在 Android 侧由本 ADR 决策 3 取代，「移动端不在桌面发布范围」仍为真）；ADR-0047（命令注册扫描面不变，决策 6）；CI 基线 `.github/workflows/build.yml`；根 README「安装」节（随实施同步）

## 背景

ADR-0066 建立三平台桌面发布矩阵时，明确把移动端排除在范围外。现需 Android 安装包。Tauri v2 已支持
Android 目标，但本仓库移动工程未初始化（`src-tauri/gen/` 无 `android`），且探查发现三处移动端硬伤：
`tauri-plugin-window-state` 2.4.1 在 Android 目标下整个 crate 被 cfg 剔除，注册处（`lib.rs` 插件注册）
必然编译失败；`api_server` 无条件启动并绑定 `127.0.0.1:9527`，失败即 `expect` 崩溃；启动期 DB 初始化
失败的兜底 `blocking_show` 在 Android 主线程会死锁。渠道上 iOS 分发硬依赖付费 Apple Developer Program
账号（当前没有），Android 可自签名后直装。范围、渠道、签名与门禁经 grilling 逐层定稿如下。

## 决策

### 1. 平台范围：仅 Android；iOS 整体暂缓

- iOS 分发（TestFlight、ad hoc、App Store）均以付费开发者账号为前提，无账号时 CI 只能产出
  **不可安装**的产物。否决「CI 留 iOS 编译验证绿灯」：需维护一整套 `gen/apple` 工程与更长的
  macOS runner 时长，且无真实用户、无验证设备，绿灯无意义。待有账号时连同 TestFlight 路线另立 ADR。

### 2. 分发与 CI 拓扑：并入现有 `build.yml` 矩阵，产物汇入单一 GitHub Release

- release 矩阵新增 Android 行（`ubuntu-latest`），构建 universal APK（Tauri 默认四 ABI 合一，
  产物单一）；`upload-artifact` 后由既有 publish job 汇入**同一个** GitHub Release，复用 CHANGELOG
  说明提取；`workflow_dispatch` 试跑机制自动继承。
- **备选与否决**：独立 `mobile.yml`——会重新引入 ADR-0066 决策 3 特意消灭的多 job 竞争建 Release
  问题，publish 拓扑需另做。

### 3. 签名：自签名 keystore，先备份、后配 secrets、再启用

- keytool 本地生成（RSA 2048+、有效期 10000 天），keystore 是「丢失即永远无法给已装用户发升级」
  的资产，流程强制：**口令进密码管理器 + jks 文件异地备份 → 配置 GitHub Actions secrets → 才启用发版**。
- 取代 ADR-0066 决策 4 的 Android 侧；桌面三平台维持不签名。
- **备选与否决**：Play 应用签名（绑定 Play 渠道，本轮无 Play 分发）；不签名（release APK 无法安装，
  签名是直装的硬前提）。

### 4. versionCode：semver 公式派生，CI 注入

- `versionCode = major×10⁶ + minor×10³ + patch`（如 v0.6.3 → 6003），CI 从 tag 计算并在构建时注入，
  零手工。本条为唯一口径。
- **备选与否决**：手工维护递增（挂 run-release）——易漏、不可回溯，且 Android 升级要求 versionCode
  单调，人工记忆是最弱的保障。

### 5. 验收边界：CI 构建成功 + 真机冒烟记账主流程

- 较 ADR-0066 的「构建成功即完成」加严：Android 包作者本人即用户、会真机安装，冒烟可落地；
  UI 移动端适配不在本轮范围（另立任务）。

### 6. 移动端代码门禁：只门禁组装面，不动命令扫描面

- **window_state 桌面化**：Cargo 依赖按 target 声明 + 注册处 `#[cfg(desktop)]`（必做项，否则 Android
  编译必炸）。
- **api_server 移动端不启动**：启动调用 `#[cfg(desktop)]` 门禁——移动端没有 AI 导入的消费方，顺带
  消掉绑定失败即崩的 `expect`；模块本体仍参与编译，`#[cfg(desktop)]` 不落在任何 `#[tauri::command]`
  上，`build.rs` 扫描器（遇条件命令 panic，ADR-0047）零改动。
- **启动兜底 dialog 分平台**：桌面保留 `blocking_show`；移动端改为记日志后带错误退出，避免主线程死锁。
- 已知移动端行为差异**接受并记录、不修**：恢复备份后 `restart`/`exit` 表现为应用退出需手动重开；
  「打开日志目录」无对应语义（会失败）；备份目录选择走系统文件选择器（scoped storage）。

### 7. 工程入库：`gen/android` 提交

- `tauri android init` 生成的 `gen/android` 是签名、图标等定制的载体，且 CI 构建依赖，提交入库；
  `gen/schemas` 维持现有 gitignore。

## 后果

- 实施前置：本机当前无真实 JDK 与 Android SDK/NDK（`keytool`/`java` 为系统空壳），需安装
  Android Studio（或 brew 的 temurin + commandlinetools）；CI runner 自带，不受影响。
- 实施内容：`.github/workflows/build.yml` 矩阵扩行与签名接线、`gen/android` 初始化与入库、
  决策 6 的三处门禁、keystore 生成与备份交接；README「安装」节补 APK 与「未知来源」放行提示，
  CHANGELOG 在 `Unreleased` 下记录，均随实施 PR 同步。
- iOS 路线（TestFlight）待付费开发者账号就绪后另立 ADR，含证书/provisioning 的 secrets 方案。
