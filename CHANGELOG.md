# Changelog

本文件记录 Ledger 各版本对使用者可见的变更，格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)规则。

## [Unreleased]

### Changed

- **外观**：主题定制化（Raycast 精致工具感）——强调色改为琥珀暖橙、圆角提升至 8/12px、暗色背景分层为近黑底+细边框；收入/支出/退款语义色保持不变。评估了迁移至 Shadcn Vue + Tailwind 的路线后决定留在 Naive UI，改用 `theme-overrides`（`src/theme/overrides.ts`）定制，迁移成本（约 1.5–2 周）与收益不匹配。

## [0.2.0] - 2026-08-22

### Added

- **AI 导入**：新增 AI 系统提示词视图，可在应用内查看并一键复制 AI 导入入口提示词（供 Cursor、Claude Code 等 AI 助手使用），提示词指引 AI 通过 HTTP API 自行发现端点与获取导入约定

## [0.1.1] - 2026-08-22

### Changed

- **依赖**：升级后端 Rust 依赖，HTTP 客户端切换为 rustls

### Fixed

- **构建**：修复 clippy 警告，CI 补充后端检查

## [0.1.0] - 2026-08-22

### Added

- **账本**：首个可用版本——多币种账户、支出/收入/转账记录、分类管理（图标/颜色/排序/二级层级）、预算与报表视图
- **投资**：投资账户与股票标的、买卖交易、FIFO 卖出匹配与已实现盈亏、现价展示、东方财富全量同步
- **AI 导入**：本地 HTTP API（/api/v1）供 AI 助手幂等写入账户/分类/交易，支持批量去重与黑洞账户，附 OpenAPI 文档与导入知识说明
- **计划交易**：定时交易模块（订阅/分期/转账）
- **日志**：按天滚动日志，设置页可打开日志目录
- **发布**：GitHub Actions 构建 macOS DMG，打 v* tag 自动发布 GitHub Release

### Fixed

- **同步**：股票同步分页截断导致港股漏抓、跨市场进度条重置、启动崩溃（tokio 运行时上下文）、JPY 精度等问题
