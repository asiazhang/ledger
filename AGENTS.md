# AGENTS.md

本文件为 WARP（warp.dev）在本仓库中处理代码时提供指导。

## 项目概览

Ledger 是一个基于 Tauri 2 的桌面记账应用，前端 Vue 3 + TypeScript + Vite，后端 Rust。

> **版本状态：当前尚未发布，处于早期开发/MVP 阶段。任何数据库 schema、API、数据模型的重构或迁移都无需考虑向后兼容，可直接按最合理的方式调整。**

## 需求文档归档

需求文档（`specs/*.md`）的文件名必须使用英文。按完成/归档日期存放到英文归档目录 `specs/archive/` 下，目录结构为 `YYYY/MM/DD/`。例如 2026 年 7 月 7 日完成的需求文档归档到 `specs/archive/2026/07/07/`。

## 常用命令

开发与构建（在仓库根目录执行）：
- `npm run tauri dev` — 启动完整开发环境（Vite dev server + Rust 后端，热重载）。这是日常开发主命令。
- `npm run tauri build` — 构建发布版桌面应用（先 `npm run build` 再编译 Rust 并打包）。
- `npm run build` — 仅构建前端（等价 `vue-tsc --noEmit && vite build`，会做类型检查）。
- `npm run dev` — 仅启动前端 Vite（端口 1420），无 Rust 后端，invoke 调用会失败，仅在纯 UI 调试时使用。

类型检查与 Rust 质量检查：
- 前端类型检查：`npx vue-tsc --noEmit`（`tsconfig.json` 开启了 strict、noUnusedLocals、noUnusedParameters）。
- Rust 检查（在 `src-tauri/` 下）：`cargo clippy --all-targets --all-features`，并运行 `cargo fmt` 格式化。请修复所有 clippy 警告。
- Rust 测试（在 `src-tauri/` 下）：`cargo test --all`（71 个单元测试 + 6 个 BDD 场景）。
- BDD 测试（Rust 后端，cucumber）：`cargo test --test e2e`（在 `src-tauri/` 下）。使用 Gherkin 语法，feature 文件在 `src-tauri/tests/e2e/features/`。
- 前端测试：`npm test`（Vitest，37 个测试，4 个文件）。使用 jsdom 环境 + `@vue/test-utils`，mock `@tauri-apps/api/core` 的 `invoke`。测试文件位于 `src/__tests__/`。
  - `npm run test:watch` — watch 模式。
- 新增 Rust 业务逻辑时，考虑补充 BDD feature 场景（`src-tauri/tests/e2e/features/`）和对应的 step 定义（`src-tauri/tests/e2e/*_steps.rs`）。
- 新增前端逻辑时，补充 Vitest 测试到 `src/__tests__/`（纯函数、composables、组件均可测）。

> 注意：Vite dev server 固定占用 1420 端口（`strictPort: true`），且 Vite 配置已忽略 `src-tauri/**` 的文件监听，改 Rust 代码需靠 `tauri dev` 自身的 Rust 热重载。

## 架构要点

### Tauri IPC 数据流
后端命令集中在 `src-tauri/src/commands.rs`，每个 `#[tauri::command]` 函数在 `src-tauri/src/lib.rs` 的 `generate_handler![...]` 中注册。前端通过 `@tauri-apps/api/core` 的 `invoke` 调用，所有调用统一封装在 `src/api/index.ts` 的 `api` 对象里——新增后端命令时必须同步三处：
1. `commands.rs` 加 `#[tauri::command]` 函数；
2. `lib.rs` 的 `generate_handler!` 注册；
3. `src/api/index.ts` 加对应方法；
4. 必要时在 `src/types/index.ts` 加 TS 类型（与 `src-tauri/src/models.rs` 的 serde 结构对应，注意 `#[serde(rename = "type")]` 这类字段名映射）。

### 金额与多币种（重要约定）
- 所有金额以**整数分**存储，字段统一用 `_cents` 后缀（如 `amount_cents`、`initial_balance_cents`、`balance_cents`）。前端用 `src/types/index.ts` 的 `formatAmount(cents, currency)` 按币种 `decimal_places` 格式化展示。
- `transactions` 同时存 `amount_cents`（原始币种金额）和 `amount_native_cents`（本位币金额）。**当前 MVP 阶段二者始终相等（1:1）**，多币种汇率换算尚未实现，`exchange_rates` 表为此预留。改动金额相关逻辑时勿破坏此约定。
- 账户余额**不持久化**，由 `commands::account_balance` 实时计算：`初始余额 + 收入 - 支出 + 转入 - 转出`。转账（`kind='transfer'`）用 `account_id` 表示转出账户、`to_account_id` 表示转入账户。

### 交易类型约束
`transactions.kind` 受数据库 CHECK 约束为 `'income' | 'expense' | 'transfer'`；`categories.kind` 为 `'income' | 'expense'`。`create_transaction` 在 Rust 侧校验金额 > 0、转账必须有 `to_account_id`。前端 `TransactionForm.vue` 同样遵循：按金额正负判定为 income/expense。

### 错误处理
`src-tauri/src/error.rs` 定义 `AppError`（thiserror + serde，`#[serde(tag = "kind", content = "message")]`），序列化为 `{kind, message}` 传到前端。`Result<T>` 是 `std::result::Result<T, AppError>`。已实现 `From` 转换：`rusqlite::Error`、`std::io::Error`。新增可失败命令用 `?` 即可。

### 前端状态与路由
- 单一 Pinia store `src/stores/app.ts`（`useAppStore`）缓存 `currencies/accounts/categories`，并提供 `currencyMap/categoryMap/accountMap` 计算属性与 `expenseCategories/incomeCategories`。`loadAll()` 幂等加载，`App.vue` 在 `onMounted` 调用一次；各视图按需调用 `store.loadAccounts()` 等刷新。
- 路由用 hash 模式（`createWebHashHistory`，Tauri webview 需要），6 个视图：dashboard / transactions / accounts / reports / budget / settings。视图均懒加载。
- `@` 别名指向 `./src`（在 `vite.config.ts` 与 `tsconfig.json` 同时配置）。
- Naive UI 采用**按需 import**（非全局注册），`App.vue` 硬编码使用 `darkTheme` 暗色主题。

### 导入流程（AI 驱动）
导入**不按文件类型解析**，旧入口（`preview_import` 命令、`import_parser` 模块、`ImportView.vue`）已删除。唯一入口是本地 HTTP API `/api/v1`（基础地址 `http://127.0.0.1:9527`）：AI 编程助手读取原始文件、分析格式、把行映射为账户/分类/交易，再通过 `POST /api/v1/accounts`、`POST /api/v1/categories`、`POST /api/v1/transactions/batch` 等端点幂等写库。导入约定（列映射、转账拆分、黑洞账户、币种映射、分单位、日期、dedup）由 `GET /api/v1/import/knowledge` 以纯文本返回，供 AI 直接注入；机器可读的完整端点契约由 `GET /api/v1/openapi.json` 生成式返回（utoipa）。AI 入口提示词模板见 `src-tauri/prompts/ledger-api.md`。

## 编码约定

- Rust 代码请保持零 clippy 警告；时间戳统一用 `db::now_iso()`（UTC ISO 字符串）。
- Rust 字符串/错误信息使用中文（与现有 `AppError` 枚举、种子数据一致）。
- 前端新组件沿用 Naive UI 按需 import + `<script setup lang="ts">` 风格；金额一律走 `formatAmount`，不要在视图中手写 `/100`。
