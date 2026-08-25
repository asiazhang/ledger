# AGENTS.md

本文件为 WARP（warp.dev）在本仓库中处理代码时提供指导。

## 项目概览

Ledger 是一个基于 Tauri 2 的桌面记账应用，前端 Vue 3 + TypeScript + Vite，后端 Rust。

> **发布约定：打 git tag 即发布。** 未发布（最新 tag 之后）的数据库 schema、API、数据模型可自由修改，无需向后兼容；已发布（最新 tag 及更早）的**冻结**：迁移只增不改（变更一律新增向前迁移），API 与数据模型同样只增不改。

## 常用命令

开发与构建（在仓库根目录执行）：
- `npm run tauri dev` — 启动完整开发环境（Vite dev server + Rust 后端，热重载）。这是日常开发主命令。
- `npm run tauri build` — 构建发布版桌面应用。
- `npm run build` — 仅构建前端，含 `vue-tsc` 类型检查。
- `npm run dev` — 仅启动前端 Vite，无 Rust 后端，`invoke` 调用会失败；仅在纯 UI 调试时使用。

类型检查与 Rust 质量检查（Rust 相关命令在 `src-tauri/` 下执行）：
- 前端类型检查：`npx vue-tsc --noEmit`（`tsconfig.json` 开启了 strict、noUnusedLocals、noUnusedParameters）。
- Rust 检查：`cargo clippy --all-targets --all-features`，并运行 `cargo fmt` 格式化。请修复所有 clippy 警告。
- Rust 单元测试：`cargo test --all`。
- BDD 测试（Rust 后端，cucumber）：`cargo test --test e2e`。feature 文件在 `src-tauri/tests/e2e/features/`。
- 前端测试：`npm test`（Vitest）。使用 jsdom 环境 + `@vue/test-utils`，mock `@tauri-apps/api/core` 的 `invoke`；测试文件位于 `src/__tests__/`。
- 新增 Rust 业务逻辑时，同步补充 BDD feature 场景（`src-tauri/tests/e2e/features/`）与对应的 step 定义（`src-tauri/tests/e2e/*_steps.rs`）。
- 新增前端逻辑时，补充 Vitest 测试到 `src/__tests__/`（纯函数、composables、组件均可测）。

> 注意：Vite 配置已忽略对 `src-tauri/**` 的文件监听，改 Rust 代码不会触发前端热更新——Rust 热重载由 `tauri dev` 自身处理。

## 架构要点

### Tauri IPC 数据流
后端命令集中在 `src-tauri/src/commands.rs`，前端统一经 `src/api/index.ts` 的 `api` 对象调用（`@tauri-apps/api/core` 的 `invoke`）——新增后端命令时按下表同步（1–3 必做，4 按需）：
1. `commands.rs` 加 `#[tauri::command]` 函数；
2. `lib.rs` 的 `generate_handler!` 注册；
3. `src/api/index.ts` 加对应方法；
4. 必要时在 `src/types/index.ts` 加 TS 类型（与 `src-tauri/src/models/`（按领域拆分，`mod.rs` 统一重导出）的 serde 结构对应，注意 `#[serde(rename = "type")]` 这类字段名映射）。

### 金额与多币种（重要约定）
- 所有金额以**整数分**存储，字段统一用 `_cents` 后缀（如 `amount_cents`、`initial_balance_cents`、`balance_cents`）。前端用 `src/types/index.ts` 的 `formatAmount(cents, currency)` 按币种 `decimal_places` 格式化展示。
- 金额口径收口在 `src-tauri/src/transaction/amount.rs` 的 **Amount 接缝**（唯一权威）：`transactions` 同时存 `amount_cents`（原始币种金额）和 `amount_native_cents`（本位币金额），折算经 `convert_to_native` 以**全局默认币种**为基准（MVP 阶段多币种汇率 1:1，`exchange_rates` 表为此预留；非默认币种缺汇率时报错不静默混币种）；kind→度量系数矩阵同时驱动 SQL 聚合片段（`*_expr`）与行级 `signed_amount`，改口径只改模块内矩阵一处。**改动金额相关逻辑时须经模块接口，不要另写口径表达式。**
- 账户余额**不持久化**，由 `commands::account_balance` 实时计算，口径 = `account_flow` 度量：`初始余额 + 收入 − 支出 + 转入 − 转出 + 退款 − 买入 + 卖出 + 分红`（split 恒 0）。转账（`kind='transfer'`）用 `account_id` 表示转出账户、`to_account_id` 表示转入账户。

### 交易类型约束
`transactions.kind` 受数据库 CHECK 约束为 **8 种**：`'income' | 'expense' | 'transfer' | 'refund' | 'buy' | 'sell' | 'dividend' | 'split'`（真源为 `transaction::amount::Kind` 枚举）；`categories.kind` 为 `'income' | 'expense'`。校验（金额 > 0、转账必须有 `to_account_id`、退款继承原支出账户/币种/分类）与落库（含本位币折算、id 与审计字段生成）收口在 `src-tauri/src/transaction/writer.rs` 的 **Writer 接缝**（`normalize` / `insert_row` / `update_row`）：命令层 `create_transaction` / `update_transaction_internal`、买入/卖出行、定时引擎、批量导入全部经它落库，列清单不在此之外重复。前端 `TransactionForm.vue` 同样遵循：按金额正负判定为 income/expense。

### 错误处理
`src-tauri/src/error.rs` 定义 `AppError`（thiserror + serde，`#[serde(tag = "kind", content = "message")]`），序列化为 `{kind, message}` 传到前端。`Result<T>` 是 `std::result::Result<T, AppError>`。已实现 `From` 转换：`rusqlite::Error`、`std::io::Error`。新增可失败命令用 `?` 即可。

### 前端状态与路由
- 参考数据（`currencies/accounts/categories`）的**单一来源**是 `src/stores/reference.ts` 的 `useReferenceStore`：持有三张参考表、全部派生映射（`currencyMap/accountMap/categoryMap`）、分类树逻辑（`rootCategories/expenseCategories/incomeCategories/categoryChildren/categoryPath/treeCategoryOptions`）与加载函数（`loadAll/loadCurrencies/loadAccounts/loadCategories`）。消费端迁移分批进行中：`useAppStore`（`src/stores/app.ts`）当前仍暴露同套参考数据 getters（delegate 到 `useReferenceStore`，共享同一份状态），新消费者优先从 `useReferenceStore` 读取；迁移完成后 `useAppStore` 将收缩为纯 UI 设置 store（`theme/defaultCurrency/backupDir/backupMaxCount`）。`loadAll()` 由 `App.vue` 在 `onMounted` 调用一次；各视图按需调用 `store.loadAccounts()` 等刷新。
- 路由用 hash 模式（`createWebHashHistory`，Tauri webview 需要），6 个视图：dashboard / transactions / accounts / reports / budget / settings。
- `@` 别名指向 `./src`（在 `vite.config.ts` 与 `tsconfig.json` 同时配置）。
- Naive UI 采用**按需 import**（非全局注册），`App.vue` 硬编码使用 `darkTheme` 暗色主题。

### 导入流程（AI 驱动）
导入**不按文件类型解析**。唯一入口是本地 HTTP API `/api/v1`（基础地址 `http://127.0.0.1:9527`）：AI 编程助手通过 `POST /api/v1/accounts`、`POST /api/v1/categories`、`POST /api/v1/transactions/batch` 等端点幂等写库。导入约定（列映射、转账拆分、黑洞账户、币种映射、分单位、日期、dedup）由 `GET /api/v1/import/knowledge` 以纯文本返回，供 AI 直接注入；机器可读的完整端点契约由 `GET /api/v1/openapi.json` 生成式返回（utoipa）。AI 入口提示词模板见 `src-tauri/prompts/ledger-api.md`。

## 编码约定

- Rust 时间戳统一用 `db::now_iso()`（UTC ISO 字符串）。
- Rust 字符串/错误信息使用中文（与现有 `AppError` 枚举、种子数据一致）。
- 前端新组件沿用 Naive UI 按需 import + `<script setup lang="ts">` 风格；金额一律走 `formatAmount`，不要在视图中手写 `/100`。
