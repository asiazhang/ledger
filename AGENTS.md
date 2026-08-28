# AGENTS.md

给 AI 编程助手的指导，只讲「原则」与「去哪查」；事实层（schema、路由、命令名）以环境与下列文档为准。

## 文档导航

动手前先定位到对应层：

- **`CONTEXT-MAP.md`** — 领域词汇表地图：领域词汇表（叙述与决策归 `docs/adr/`）按域拆分为集中存放于 `docs/contexts/` 的 `CONTEXT-*.md` 分域文件。动手前先读地图，再选读与改动主题相关的分域词汇表与相关 ADR；**与代码行为冲突时以代码为准并同步修正词汇表。**
- **`docs/adr/`** — 决策记录（文件名即编号与主题）。改到某区域前先读该区域的 ADR。
- **`docs/agents/`** — 专项操作指引（issue-tracker 用 `gh`、triage 标签、domain 文档消费方式）。
- **`docs/model/`** — 数据模型。
- **`scripts/`** — 一键脚本，用法见各脚本头部注释（含 README 未提及的操作）。

## 项目概览

Ledger：Tauri 2 桌面记账应用，前端 Vue 3 + TypeScript、后端 Rust。后端命令集中在 `src-tauri/src/commands/`，前端经 `src/api/index.ts` 的 `api` 对象 invoke。

> **发布约定：打 git tag 即发布。** 未发布（最新 tag 之后）的 schema、API、数据模型可自由修改；已发布（最新 tag 及更早）**冻结**：迁移只增不改（变更一律新增向前迁移），API 与数据模型只增不改。

## 工作流约定

- **调用 `/implement` skill 实施任何改动必须在独立的 git worktree 中进行**：`git worktree add` 创建，完成后在工作树内提交。这样主检出目录始终保持干净，实验性改动不污染当前状态。
- **worktree 内跑前端类型检查需先软链 node_modules**：`git worktree add` 建立的独立检出**不含 `node_modules`**，此时直接跑 `./scripts/check.sh`（或 `npx vue-tsc`）会让 npx 走缓存里与项目不匹配的 `typescript`，报 `ERR_PACKAGE_PATH_NOT_EXPORTED` 即此因。解决：软链主检出的依赖即可——`ln -s <主检出绝对路径>/node_modules <worktree>/node_modules`，随后 `vue-tsc --noEmit` 与 `check.sh` 正常（node_modules 为 gitignore，软链不进提交）。

## 金额与多币种

- 金额以**整数分**存储，字段统一 `_cents` 后缀；前端展示一律走 `formatAmount(cents, currency)`（`src/types/index.ts`），**不要手写 `/100`**。
- 金额口径收口在 `src-tauri/src/transaction/amount.rs` 的 **Amount 接缝**（唯一权威，符号归属详见核心交易域词汇表 `docs/contexts/CONTEXT-core.md` 的 Transaction Kind Mapping）：`transactions` 同时存 `amount_cents`（原始币种）与 `amount_native_cents`（本位币折算）。改动金额逻辑只改接缝内 kind→度量系数矩阵一处，矩阵同时驱动 SQL 聚合片段与行级 `signed_amount`——另写口径表达式等于造出第二个口径。
- 账户余额**不持久化**，实时计算：`commands::accounts::list_account_balances`，口径即 `account_flow` 度量（各 kind 符号见上面矩阵）。转账 `kind='transfer'` 用 `account_id` 转出、`to_account_id` 转入。

## 交易写入（行为层约束）

`transactions.kind` 是**闭集 8 种**：`income | expense | transfer | refund | buy | sell | dividend | split`（真源 `transaction::amount::TransactionKind`；注意 `categories.kind` 仅 `income | expense`）。

- 校验与落库统一走 Writer 接缝（`src-tauri/src/transaction/writer.rs`，`normalize` / `insert_row` / `update_row`）：所有写路径（命令层、买入/卖出行、定时引擎、批量导入）都经它。
- 每类 kind 的行为（校验/归一化/副作用/回退）收敛在 `commands::transactions::behavior` 行为层（`plan → apply / revert` 单点分派）：通用 kind 走 Writer 接缝，buy/sell 委托 `commands::investment` 的 `prepare / apply / revert`，dividend/split 显式「暂不支持」拒绝。改动写入行为就加在此分派上，所有写路径自然走到。

## 前端状态

- 参考数据（currencies/accounts/categories）单一来源是 `src/stores/reference.ts` 的 `useReferenceStore`（三张参考表 + 派生映射 + 分类树 + 失效信号，细节见参考数据与设置域词汇表 `docs/contexts/CONTEXT-reference-settings.md`「参考数据」条目）：读取走 `ensureFresh()` / 强制重拉走 `refresh()`，并随 `ledger:changed` 失效信号自动重拉——绕开它会躲过失效机制。
- `useAppStore`（`src/stores/app.ts`）是**纯 UI 设置 store**（theme / defaultCurrency / backupDir / backupMaxCount）。
- 路由 hash 模式（Tauri webview 需要），视图与路由以 `src/router/` 代码为准；README/词汇表若不同步，同步修正文档。
- **应用配置归口（ADR-0017）**：前端独享消费的设备偏好存 localStorage；后端消费或随 Backup/Restore 迁移的配置与运行时状态统一存 `app_settings` KV 表（读写经 `src-tauri/src/settings.rs` 枚举收口，key 规范 `<feature>.<name>`，对外 IPC 保持领域命令形状）。库外配置文件的**唯一例外**是 DataLocation 引导指针文件（ADR-0018）：建连前必须可读，进不了库内。

## AI 导入流程

AI 驱动的导入**不按文件类型解析**，唯一入口是本地 HTTP API `/api/v1`（基础地址 `http://127.0.0.1:9527`），端点幂等写库。导入约定与端点契约分别由 `GET /api/v1/import/knowledge`（纯文本）与 `GET /api/v1/openapi.json` 生成式返回；**AI 入口提示词模板见 `src-tauri/prompts/ledger-api.md`**（新需求先读它）。术语与边界见 AI 导入域词汇表 `docs/contexts/CONTEXT-ai-import.md` 的 AI API / ImportDedup / IdempotencyKey 等条目。

## 编码约定

- Rust 时间戳统一用 `db::now_iso()`（UTC ISO 字符串）。
- Rust 字符串/错误信息使用中文（与 `AppError` 枚举、种子数据一致）。
- 前端组件沿用 Naive UI 按需 import + `<script setup lang="ts">` 风格。
- **文件命名约定**：前端多词 `.ts` 模块一律 kebab-case（如 `view-state.ts`）；`.vue` 组件保持 PascalCase；composables 保持 `useXxx` camelCase；Rust 侧遵循 cargo 惯例 snake_case。测试文件跟随被测文件命名（`<被测文件名>.test.ts`）。跨平台考虑：普通模块全小写，避免大小写敏感文件系统上的歧义。
- **新增后端命令**：`commands.rs`（`src-tauri/src/commands/`）加 `#[tauri::command]` 函数 → `lib.rs` 的 `generate_handler!` 注册 → `src/api/index.ts` 加方法 → 必要时 `src/types/index.ts` 加 TS 类型（对应 `src-tauri/src/models/` serde 结构，注意 `#[serde(rename = "type")]` 字段映射）。

## 测试

- 新增 Rust 业务逻辑：补充 BDD 场景到 `src-tauri/tests/e2e/features/` 与对应 step 定义（`src-tauri/tests/e2e/*_steps.rs`）。
- 新增前端逻辑：补充 Vitest 测试到 `src/__tests__/`（纯函数、composables、组件均可测）。
- 质量门槛即 `./scripts/check.sh` 的覆盖范围（vue-tsc --noEmit、cargo clippy --all-targets --all-features、cargo fmt 无警告）；单测跑法见 `package.json` scripts 与脚本头部注释。
- 注意：Vite 配置已忽略对 `src-tauri/**` 的文件监听，改 Rust 代码不会触发前端热更新（Rust 热重载由 `tauri dev` 自身处理）。
