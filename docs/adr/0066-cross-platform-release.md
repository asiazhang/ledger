# ADR 0066: 跨平台发布——Windows/Linux 入发布矩阵、单 Release 汇总、全平台暂不签名

- 状态：已接受
- 日期：2026-09-04
- 作者：Ledger 项目
- 关联：根 README「安装」节；发布流程配置 `.run-release.json`（run-release）；CI 基线 `.github/workflows/build.yml`

## 背景

此前唯一发布产物是 macOS（Apple Silicon）DMG：`v*` tag 触发 GitHub Actions 在 `macos-latest` 上构建，DMG 上传到 GitHub Release（release job 顺手建 Release）。需要让 Windows、Linux 使用者也能直接安装，经设计评审确定跨平台发布的范围、基建与验证边界。版本推进流程（run-release：三处版本文件 + CHANGELOG + tag）保持不变。

## 决策

### 1. 平台范围：新增 Windows x64（NSIS）与 Linux x64（deb + AppImage），macOS 维持 arm64

- Windows 只出 **NSIS 安装器（`.exe`）**；Linux 出 **`.deb` + `.AppImage`**（前者覆盖 Debian/Ubuntu 系，后者覆盖其余发行版）。
- **备选与否决**：MSI——需 WiX 工具链且对个人分发无额外收益；macOS Intel——无目标用户与验证设备；Windows/Linux arm64——无验证渠道且拉长构建时间，需求出现时再加矩阵；移动端——不在桌面发布范围。

### 2. 构建基建：GitHub Actions 矩阵，不本地构建

- Tauri 不支持从 macOS 交叉编译出 Windows/Linux 安装包，跨平台产物只能在对应系统上构建；CI 已有 Ubuntu 编译 Rust 后端的检查先例（backend job），矩阵化是水到渠成。

### 3. 发布拓扑：矩阵并行构建 → artifact → 单一 publish job 汇总建 Release

- 现结构「DMG job 顺手建 Release」在多平台并行下会竞争创建；改为三个平台 job 各自 `upload-artifact`，收尾 publish job 下载全部产物、提取 CHANGELOG 条目（逻辑从现 release job 原样搬入）、创建**唯一的** GitHub Release。
- 各平台 job 以 `--bundles` 圈定产物（dmg / nsis / deb,appimage），`tauri.conf.json` 的 `targets: "all"` 不动；release job 增加 `workflow_dispatch`，正式 tag 前先手动试跑一轮。
- **备选与否决**：`tauri-apps/tauri-action` 全托管——更标准，但要重写现有 job 并适配 CHANGELOG 说明提取，改动面更大；留作未来选项。

### 4. 签名：全平台暂不签名

- 与 macOS 现状一致（DMG 一直未签名、未公证）；正式签名需要持续花钱（Windows 代码签名证书或 Azure Trusted Signing，macOS 公证需 Apple Developer 年费）。Release 说明写明 SmartScreen / Gatekeeper 放行提示。有外部用户或签名预算时重开决策并另立 ADR。

### 5. 验证边界：完成标准 = CI 三平台构建成功

- 眼下没有 Windows/Linux 真机或虚拟机（Apple Silicon 上的虚拟机只能跑 ARM 客户系统，验证 x64 包结论打折），冒烟验证无从落地。首个跨平台版本在 Release 说明标注「未经真机验证」；运行缺陷由使用者反馈后单独修，不阻塞发布基建落地。

## 后果

- `.github/workflows/build.yml` 的 release job 改为「平台矩阵 + publish 收尾」结构（后续任务落实）。
- 根 README「安装」节列各平台产物与未签名放行提示。
- 词汇表不新增条目：构建/发布是工程基建，不是应用域词汇。
- 缓行清单：macOS Intel 与各平台 arm64、MSI、签名与公证、应用内自动更新（Tauri updater）、移动端。
