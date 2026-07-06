# Ledger

Ledger 是一个基于 Tauri 2 的桌面记账应用，目标是在本地安全、快速地管理个人账目。目前处于早期 MVP 阶段，数据库 schema 与 API 仍在快速迭代中。

## 技术栈

- **桌面框架**：Tauri 2（Rust 后端 + Webview 前端）
- **前端**：Vue 3 + TypeScript + Vite
- **UI 组件**：Naive UI（按需引入，暗色主题）
- **状态管理**：Pinia
- **路由**：Vue Router（hash 模式）
- **图表**：Chart.js + vue-chartjs
- **后端语言**：Rust
- **数据库**：SQLite（通过 rusqlite 访问）
- **数据迁移**：Rust 嵌入式 SQL 迁移（`src-tauri/migrations/`）

## 主要功能

- 仪表盘：收支概览与图表可视化
- 交易记录：收入、支出、转账三类交易增删改查
- 账户管理：账户列表与实时余额计算
- 分类管理：收入/支出分类
- 报表分析：按分类、时间维度统计
- 预算：预算设置与跟踪（开发中）
- 数据导入：CSV / Excel 导入预览与写入
- 设置：多币种、默认配置等

## 常用命令

在仓库根目录执行：

```bash
# 启动完整开发环境（Vite + Rust 后端，热重载）
npm run tauri dev

# 仅构建前端（包含 TypeScript 类型检查）
npm run build

# 构建发布版桌面应用
npm run tauri build
```

## 架构要点

- Tauri IPC 是前后端唯一通信方式：后端命令定义在 `src-tauri/src/commands.rs`，并在 `src-tauri/src/lib.rs` 注册；前端统一通过 `src/api/index.ts` 调用 `invoke`。
- 所有金额以**整数分**存储，字段名以 `_cents` 结尾；展示时通过 `src/types/index.ts` 的 `formatAmount` 按币种精度格式化。
- 当前 MVP 阶段多币种汇率为 1:1，`exchange_rates` 表为后续汇率换算预留。
- 账户余额不持久化，由后端实时根据交易汇总计算。
- 错误统一使用 `AppError` 序列化为 `{ kind, message }` 传递到前端。

更多细节请参见 `AGENTS.md`。
