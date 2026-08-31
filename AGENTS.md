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

> **发布约定：打 git tag 即发布。** 未发布（最新 tag 之后）的 schema、API、数据模型可自由修改；已发布（最新 tag 及更早）：**迁移文件可就地修改**（已执行过该迁移的存量库保持原 schema，与新装库的 schema 分叉被接受），**API 与数据模型只增不改**。凡就地修改已发布迁移，发布（打 tag）时必须以两级 BREAKING 标记承载：被改迁移文件头部加就地修改注记（指向 CHANGELOG 版本）+ CHANGELOG 对应版本下加 BREAKING 条目。

## 工作流约定

- **调用 `/implement` skill 实施任何改动必须在独立的 git worktree 中进行**：`git worktree add` 创建，完成后在工作树内提交。这样主检出目录始终保持干净，实验性改动不污染当前状态。
- **worktree 内跑前端检查前先 `pnpm install`**：`git worktree add` 建立的独立检出**不含 `node_modules`**，此时直接跑 `./scripts/check.sh`（或 `pnpm exec vue-tsc`）会因本地缺少依赖而失败。pnpm 全局 store + 硬链接让 worktree 内安装秒级完成，无需 npm 时代「软链主检出 node_modules」的 hack（node_modules 为 gitignore，不入提交）。
- **文档守门（三层标尺）**：写分域词汇表与模型文档时，实现坐标（路径、函数名、清单、DDL、正则、公式）一律不进文档——「甲删乙留丙留」标尺见 `CONTEXT-MAP.md`「结构约定」；`scripts/check-docs.sh` 的代码坐标扫描命中即门槛失败。

## 金额与多币种

- 金额以**整数分**存储，字段统一 `_cents` 后缀；前端展示一律走 `formatAmount(cents, currency)`（`src/types/index.ts`），**不要手写 `/100`**。
- 金额口径收口在 `src-tauri/src/transaction/amount.rs` 的 **Amount 接缝**（唯一权威，符号归属详见核心交易域词汇表 `docs/contexts/CONTEXT-core.md` 的 Transaction Kind Mapping）：`transactions` 同时存 `amount_cents`（原始币种）与 `amount_native_cents`（本位币折算）。改动金额逻辑只改接缝内 kind→度量系数矩阵一处，矩阵同时驱动 SQL 聚合片段与行级 `signed_amount`——另写口径表达式等于造出第二个口径。
- 账户余额**不持久化**，实时计算：`commands::accounts::list_account_balances`，口径即 `account_flow` 度量（各 kind 符号见上面矩阵）。转账 `kind='transfer'` 用 `account_id` 转出、`to_account_id` 转入。

## 交易写入（行为层约束）

`transactions.kind` 是**闭集 8 种**：`income | expense | transfer | refund | buy | sell | dividend | split`（真源 `transaction::amount::TransactionKind`；注意 `categories.kind` 仅 `income | expense`）。

- 校验与落库统一走 Writer 接缝（`src-tauri/src/transaction/writer.rs`，`normalize` / `insert_row` / `update_row`）：所有写路径（命令层、买入/卖出行、定时引擎、批量导入）都经它。
- 每类 kind 的行为（校验/归一化/副作用/回退）收敛在 `commands::transactions::behavior` 行为层（`plan → apply / revert` 单点分派）：通用 kind 走 Writer 接缝，buy/sell 委托 `commands::investment` 的 `prepare / apply / revert`，dividend/split 显式「暂不支持」拒绝。改动写入行为就加在此分派上，所有写路径自然走到。

## 前端状态

- 参考数据（currencies/accounts/categories/merchants）单一来源是 `src/stores/reference.ts` 的 `useReferenceStore`（四张参考表 + 派生映射 + 分类树 + 失效信号，细节见参考数据与设置域词汇表 `docs/contexts/CONTEXT-reference-settings.md`「参考数据」条目）：读取 = 直接消费响应式状态（self-init + `ledger:changed` 失效信号自动重拉保证新鲜）；仅少数场景显式 `await refresh()` 强制重拉——绕开它会躲过失效机制。
- `useAppStore`（`src/stores/app.ts`）是**应用设置 store**：设备偏好（theme / defaultCurrency / backupDir / backupMaxCount / autoExecutionEnabled）单源 localStorage；后端消费的镜像推送（备份目录、自动执行开关）收口在 `useDevicePreferenceSync`，由应用根组件挂载一次。
- 路由 hash 模式（Tauri webview 需要），视图与路由以 `src/router/` 代码为准；README/词汇表若不同步，同步修正文档。
- **应用配置归口（ADR-0017）**：前端独享消费的设备偏好存 localStorage；后端消费或随 Backup/Restore 迁移的配置与运行时状态统一存 `app_settings` KV 表（读写经 `src-tauri/src/settings.rs` 枚举收口，key 规范 `<feature>.<name>`，对外 IPC 保持领域命令形状）。库外配置文件的**唯一例外**是 DataLocation 引导指针文件（ADR-0018）：建连前必须可读，进不了库内。

## AI 导入流程

AI 驱动的导入**不按文件类型解析**，唯一入口是本地 HTTP API `/api/v1`（基础地址 `http://127.0.0.1:9527`），端点幂等写库。导入约定与端点契约分别由 `GET /api/v1/import/knowledge`（纯文本）与 `GET /api/v1/openapi.json` 生成式返回；**AI 入口提示词模板见 `src-tauri/prompts/ledger-api.md`**（新需求先读它）。术语与边界见 AI 导入域词汇表 `docs/contexts/CONTEXT-ai-import.md` 的 AI API / ImportDedup / IdempotencyKey 等条目。

## 编码约定

- Rust 时间戳统一用 `db::now_iso()`（UTC ISO 字符串）。
- Rust 字符串/错误信息使用中文（与 `AppError` 枚举、种子数据一致）。
- 前端组件沿用 Naive UI 按需 import + `<script setup lang="ts">` 风格。
- **弹层关闭语义**：新弹窗一律用 `AppModal`（`src/components/AppModal.vue`，默认遮罩点击不关）不直接用 NModal；useDialog 调用点改用 `useAppDialog` 并显式传 `maskClosable: false`（语义详见界面状态与交互域词汇表 `docs/contexts/CONTEXT-ui-interaction.md`「弹层关闭语义」）。
- **弹层封装与快捷键抑制（ADR-0035）**：应用内一切弹层（NSelect/NTreeSelect/NDatePicker/NDropdown/NPopconfirm/NModal/useDialog）一律经 `src/components/App*.vue` 封装或 `useAppDialog` 使用——封装接入弹层注册表（显式上报开/关）驱动快捷键抑制，绕过封装会脱离抑制。封装刻意不声明 `show` prop（Vue 对可选 Boolean prop 的缺席转型会把非受控用法变成受控关闭），`:show`/`@update:show` 走 attrs 透传；新增弹层形态时在对应 App* 封装内接线，不回全局 DOM 嗓探。
- **文件命名约定**：前端多词 `.ts` 模块一律 kebab-case（如 `view-state.ts`）；`.vue` 组件保持 PascalCase；composables 保持 `useXxx` camelCase；Rust 侧遵循 cargo 惯例 snake_case。测试文件命名：默认 `<被测文件名>.test.ts`；单文件超约 800 行时允许拆为以被测文件命名的目录、内部按主题命名（先例 `src/__tests__/TransactionsView/`：`pagination.test.ts`、`filtering.test.ts`、`transfer-row.test.ts` 等，目录名沿用被测文件名大小写，共享辅助收进目录内 `common.ts`）。跨平台考虑：普通模块全小写，避免大小写敏感文件系统上的歧义。
- **新增后端命令**：`src-tauri/src/commands/` 域内加 `#[tauri::command]` 函数即完成注册（build.rs 扫描注解生成清单，ADR-0047，lib.rs 零改动；只认裸注解 + 紧随 pub fn / pub async fn，其他形态构建报错）→ `src/api/index.ts` 加方法 → 必要时 `src/types/index.ts` 加 TS 类型（对应 `src-tauri/src/models/` serde 结构，注意 `#[serde(rename = "type")]` 字段映射）。Rust 命令集与 TS 调用面的双向全等由 `node scripts/check-commands.js` 拦截（挂 check.sh 与 CI，双向孤儿非零退出）。
- **界面文案与 i18n（ADR-0049）**：新增用户可见文案一律写入 `src/i18n/locales/<locale>/<域>.json` 并经 `@/i18n` 的 `t()` 引用，不再新增硬编码文案；语言判定链与切换收口在 `src/i18n/`，偏好为轻量设置项；各 locale key 集合全等由 `node scripts/check-i18n-keys.js` 拦截（挂 check.sh 与 CI，双向孤儿非零退出）。刻意不随语言：拼音排序/搜索、周起始日（周一）、ISO 日期格式、输入解析、导出/备份内容。
- **错误码（ADR-0050）**：新增用户可见错误条件用码化构造器（`AppError::coded` / `codedp` / `coded_not_found`，码 `<域>.<条件>` kebab-case 全仓唯一），中文 message 原样保留；码的 zh/en 模板同步补进 `src/i18n/locales/{zh-CN,en-US}/errors.json`（前端按码插值、无码透传），漏翻由 key 全等门槛拦截。序列化只增不改：不手写错误 JSON 拼接、不改 `kind`/`message` 形态。

## 测试

- 新增 Rust 业务逻辑：补充 BDD 场景到 `src-tauri/tests/e2e/features/` 与对应 step 定义（`src-tauri/tests/e2e/*_steps.rs`）。仅 HTTP 端点层的行为（经 IPC 不可达，先例 #296/#304）以 `src-tauri/tests/api_server/` 集成测试承载，不重复建 BDD 场景。
- 新增前端逻辑：补充 Vitest 测试到 `src/__tests__/`（纯函数、composables、组件均可测）。
- **BDD world 字段准入**（issue #316）：`LedgerWorld` 只收「先前步骤写入、后续步骤读取」的跨步骤状态；单个步骤函数内自产自销的降为局部变量，从未被后续步骤读取的状态不得入 world。
- 质量门槛即 `./scripts/check.sh` 的覆盖范围（vue-tsc --noEmit、cargo clippy --all-targets --all-features、cargo fmt 无警告）；单测跑法见 `package.json` scripts 与脚本头部注释。
- 注意：Vite 配置已忽略对 `src-tauri/**` 的文件监听，改 Rust 代码不会触发前端热更新（Rust 热重载由 `tauri dev` 自身处理）。
