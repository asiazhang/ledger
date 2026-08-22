# Changelog

本文件记录 Ledger 各版本对使用者可见的变更，格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)规则。

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
