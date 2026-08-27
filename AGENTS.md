# AGENTS.md

本文件为 AI 编程助手（WARP 等）在本仓库中处理代码时提供指导。只讲「原则」与「去哪查」——事实层（schema、路由、命令名）由环境与文档导航提供，不在本文件重复。

## 文档导航

本仓库按职责分四层文档。动手前先定位到对应层，不重复叙述、不猜测：

- **`CONTEXT.md`** — 领域术语与架构叙述（单一来源）。8 种交易 kind、Amount/Writer/行为层接缝、reference store、AI API、黑洞账户、投资域等概念的定义与边界都在这里。**与代码行为冲突时以 CONTEXT.md 为准并同步修正它。**
- **`docs/adr/`** — 决策记录（ADR-0012 参考数据失效、ADR-0013 行为层分派 + investment 接缝、ADR-0004 搜索等）。改到相关区域先读对应 ADR。
- **`docs/agents/`** — 专项操作指引（issue-tracker 用 `gh`、triage 标签、domain 文档消费方式）。
- **`docs/model/`、`docs/specs/`** — 数据模型与规格。
- **`scripts/`** — 一键脚本（`check.sh` / `test.sh` / `build.sh` / `lint-fix.sh` / `db.sh` / `clean.sh` 等，含 `README` 未提及的常用操作见脚本头部注释）。

## 项目概览

Ledger：Tauri 2 桌面记账应用。前端 Vue 3 + TypeScript + Vite（Naive UI 按需 import），后端 Rust（命令集中在 `src-tauri/src/commands/`，前端经 `src/api/index.ts` 的 `api` 对象 invoke）。

> **发布约定：打 git tag 即发布。** 未发布（最新 tag 之后）的 schema、API、数据模型可自由修改；已发布（最新 tag 及更早）**冻结**：迁移只增不改（变更一律新增向前迁移），API 与数据模型只增不改。

## 金额与多币种（重要约定）

- 金额以**整数分**存储，字段统一 `_cents` 后缀；前端展示一律走 `formatAmount(cents, currency)`（`src/types/index.ts`），**不要手写 `/100`**。
- 金额口径收口在 `src-tauri/src/transaction/amount.rs` 的 **Amount 接缝**（唯一权威）：`transactions` 同时存 `amount_cents`（原始币种）与 `amount_native_cents`（本位币，按全局默认币种折算）；kind→度量系数矩阵同时驱动 SQL 聚合片段与行级 `signed_amount`。**改动金额逻辑须经模块接口，不要另写口径表达式。**
- 账户余额**不持久化**，实时计算：`commands::accounts::list_account_balances`，口径 = `account_flow`（初始余额 + 收入 − 支出 + 转入 − 转出 + 退款 − 买入 + 卖出 + 分红，split 恒 0）。转账 `kind='transfer'` 用 `account_id` 转出、`to_account_id` 转入。

## 交易写入（行为层约束）

`transactions.kind` 有 **8 种**：`income | expense | transfer | refund | buy | sell | dividend | split`（真源 `transaction::amount::TransactionKind`；`categories.kind` 仅 `income | expense`）。

- 校验与落库收口在 `src-tauri/src/transaction/writer.rs` 的 **Writer 接缝**（`normalize` / `insert_row` / `update_row`）：所有写路径（命令层、买入/卖出行、定时引擎、批量导入）都经它。
- **每类 kind 的行为（校验/归一化/副作用/回退）收敛在 `commands::transactions::behavior` 行为层**（`plan → apply / revert` 单点分派，issue #72）：通用 kind 走 Writer 接缝，buy/sell 委托 `commands::investment` 的 `prepare / apply / revert`（investment 对外不再暴露其它写函数），`dividend` / `split` 显式「暂不支持」拒绝。
- **改动交易写入行为须经行为层分派，不要另起 kind 分支**；改动金额口径只改 Amount 接缝内矩阵一处。

## 前端状态

- 参考数据（currencies/accounts/categories）单一来源是 `src/stores/reference.ts` 的 `useReferenceStore`（三张参考表 + 派生映射 + 分类树 + `status/version` 失效信号；`refresh()` 强制重拉、`ensureFresh()` 新鲜窗口内零 IPC，订阅 `ledger:changed` 信号自动重拉）。
- `useAppStore`（`src/stores/app.ts`）是**纯 UI 设置 store**（`theme / defaultCurrency / backupDir / backupMaxCount`），不暴露参考数据接口。
- 路由 hash 模式（Tauri webview 需要），视图与路由在 `src/router/`（以代码为准，README/CONTEXT 若不同步以代码为准并修正文档）。
- **应用配置归口（ADR-0017）**：前端独享消费的设备偏好存 localStorage；后端消费或随 Backup/Restore 迁移的配置与运行时状态统一存 `app_settings` KV 表（读写经 `src-tauri/src/settings.rs` 枚举收口，key 规范 `<feature>.<name>`），不建单行状态专表；对外 IPC 保持领域命令形状，不做通用 get/set_setting。

## AI 导入流程

AI 驱动的导入**不按文件类型解析**，唯一入口是本地 HTTP API `/api/v1`（基础地址 `http://127.0.0.1:9527`），端点幂等写库。导入约定与端点契约分别由 `GET /api/v1/import/knowledge`（纯文本）与 `GET /api/v1/openapi.json` 生成式返回；**AI 入口提示词模板见 `src-tauri/prompts/ledger-api.md`**（新需求先读它）。术语与边界见 CONTEXT.md 的 AI API / ImportDedup / IdempotencyKey 等条目。

## 编码约定

- Rust 时间戳统一用 `db::now_iso()`（UTC ISO 字符串）。
- Rust 字符串/错误信息使用中文（与 `AppError` 枚举、种子数据一致）。
- 前端组件沿用 Naive UI 按需 import + `<script setup lang="ts">` 风格。
- **新增后端命令**：`commands.rs`（`src-tauri/src/commands/`）加 `#[tauri::command]` 函数 → `lib.rs` 的 `generate_handler!` 注册 → `src/api/index.ts` 加方法 → 必要时 `src/types/index.ts` 加 TS 类型（对应 `src-tauri/src/models/` serde 结构，注意 `#[serde(rename = "type")]` 字段映射）。

## 测试

- 新增 Rust 业务逻辑：补充 BDD 场景到 `src-tauri/tests/e2e/features/` 与对应 step 定义（`src-tauri/tests/e2e/*_steps.rs`）。
- 新增前端逻辑：补充 Vitest 测试到 `src/__tests__/`（纯函数、composables、组件均可测）。
- 运行方式见 `package.json` scripts、`src-tauri/` 下的 cargo 命令，以及 `scripts/` 一键脚本（`check.sh` 质量检查、`test.sh` Rust 单测、`build.sh` 打包）。
- 质量门槛：`vue-tsc --noEmit`、`cargo clippy --all-targets --all-features`、`cargo fmt` 无警告——即 `./scripts/check.sh` 的覆盖范围。
- 注意：Vite 配置已忽略对 `src-tauri/**` 的文件监听，改 Rust 代码不会触发前端热更新（Rust 热重载由 `tauri dev` 自身处理）。
