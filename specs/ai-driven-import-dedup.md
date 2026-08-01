# AI 驱动导入 + 去重（AI-Driven Import & Dedup）

## Problem Statement

Ledger 需要把历史账本数据（当前首例为貔貅记账/`pixiu` 导出的月度 CSV，见 `~/Work/ledger-migrate/pending-migration/*.csv`）迁移进 Ledger。旧方案是按文件类型（`.csv`/`.xlsx`/`.xls`）+ 表头列名模糊匹配解析，既写不清第三方 APP 的专有格式（Pixiu 的 `流入金额/流出金额` 双金额列、`资金账户` 内嵌 `→` 转账、中文币种名 `人民币`/`港币`），也无法扩展到"任意来源"的任意格式。

同时，迁移最怕的是**重复导入**——同一份文件被跑两次，账本被污染，且当前代码对"重复"没有任何防线：`transactions` 无业务字段唯一约束，`create_transactions_internal` 逐条裸插入，`/api/v1/accounts` 和 `/api/v1/categories` 的 create 也是裸 INSERT 零去重。

## Solution

放弃"按文件类型提供解析器"的路线，改为**AI 驱动导入**：

- 删掉旧导入路径（`preview_import`、`import_parser`、`ImportView.vue`），`/api/v1` 成为唯一导入入口。
- AI 编程助手自己读原始文件、分析格式、把行映射为账户/分类/交易，通过 HTTP API 写入 Ledger。Ledger 端提供 AI 所需的全部枚举数据（账户、分类、币种）与幂等写库能力。
- **去重由 Ledger 后端保证**：对每条 `TransactionInput` 计算确定性内容哈希（`dedup_hash`），命中已存在（未删除）的交易则跳过并返回 `duplicate: true`，不重复写库。去重默认开启、可在导入请求中关闭。
- 引入**黑洞账户**概念：`资金账户=无` 的交易写入预置的 `is_hidden` 黑洞账户（每币种一个），作为数据修正的缓冲池，用户界面不展示。

## User Stories

1. 作为 AI 编程助手，我想读取 Pixiu 导出的 CSV 并把它的一行映射为一条 Ledger 交易，从而完成数据迁移。
2. 作为 AI 编程助手，我想用 `POST /api/v1/transactions/batch` 批量写入迁移交易，从而避免逐条调用。
3. 作为 AI 编程助手，我想在批量导入时命中已存在的交易后收到 `duplicate: true` 的结果，从而知道哪些行是重复、哪些是新增，无需重试。
4. 作为 AI 编程助手，我想通过 `dedup` 参数关闭去重（默认开启），从而在确有需要时强制重新导入。
5. 作为记账用户，我想在重复导入同一份历史文件时不产生任何重复交易，从而账本保持干净。
6. 作为记账用户，我想把 `资金账户=无` 的旧交易导入到"黑洞账户"，从而数据不丢失、且之后能手动修正归属。
7. 作为记账用户，我不想在账户列表、余额视图、下拉选择器中看到黑洞账户，从而界面不被占位账户污染。
8. 作为记账用户，我想在交易列表和报表中看到黑洞账户的交易（标注账户名"无"），从而知道哪些交易待修正，并能直接编辑改挂到真实账户。
9. 作为 AI 编程助手，我想通过 `GET /api/v1/accounts` 看到包含 `is_hidden` 标志的完整账户列表，从而能定位黑洞账户并把"无"交易挂到它上面。
10. 作为 AI 编程助手，我想用 `POST /api/v1/accounts` 按名称幂等地创建账户（同名复用已有记录），从而重跑导入不会产生重复账户。
11. 作为 AI 编程助手，我想用 `POST /api/v1/categories` 按名称幂等地创建分类（同名复用已有记录），从而重跑导入不会产生重复分类。
12. 作为 AI 编程助手，我想通过 `GET /api/v1/currencies` 拿到 `人民币→CNY`、`港币→HKD` 这类币种映射，从而无需在 prompt 里硬编码猜测币种代码。
13. 作为 AI 编程助手，我想按文档约定解析 Pixiu 的 `A → B` 转账格式为 `account_id` + `to_account_id`，从而转账能通过后端校验正确落库。
14. 作为 AI 编程助手，我想把 `x → 无` / `无 → x` 的转账映射到黑洞账户，从而不把占位符当成非法数据丢弃。
15. 作为记账用户，我想删除一条导入的交易后重跑导入能把它重新写回（已删除交易不占去重位），从而删除的意图被尊重、可重新确认导入。
16. 作为记账用户，我想在修正黑洞账户交易（改挂真实账户）后删除空的黑洞账户，从而彻底清理迁移痕迹。
17. 作为开发者，我希望编辑已导入的交易（改备注、改账户）不改变其 `dedup_hash`，从而去重身份只代表"源自某次导入"，与当前内容无关。

## Implementation Decisions

### 去重（Import Dedup）
- 给 `transactions` 表新增 `dedup_hash` 列（TEXT，可空），**不建唯一索引**。
- 去重只在**导入入口**生效：`POST /api/v1/transactions/batch`。手工 `createTransaction` 与定时交易引擎不受影响。
- 哈希算法：`sha256("date|kind|amount_cents|currency_code|account_id|to_account_id")`，`to_account_id` 缺省拼空串 `""`。竖线分隔防字段拼接歧义。
- 字段集刻意**排除 note 与 category**：AI 生成的备注/分类文本非确定性，进哈希会让重跑时哈希漂移、去重失效。
- 金额用 `amount_cents`（原始币种金额），不用 `amount_native_cents`（涉及汇率，会漂移）。
- 去重只匹配 `is_deleted = 0` 的交易：软删除的交易不参与去重，重跑会重新插入（尊重用户删除意图；也让黑洞账户清空后可重新导入）。
- `dedup_hash` 导入后**保持不变**，编辑、同步（`version`/`device_id`/LWW）无特殊处理，作为普通字段随行复制。

### API 契约
- `POST /api/v1/transactions/batch` 请求体由裸数组改为 `{ "transactions": TransactionInput[], "dedup": true }`，`dedup` 默认 `true`。
- `CreateTransactionResult` 新增 `duplicate: bool`。去重命中返回 `{ success: true, duplicate: true, id: null }`——既非新建也非失败，AI 无需重试、不应上报错误。
- `POST /api/v1/accounts` 与 `POST /api/v1/categories` 改为**按自然键幂等**：账户按 `name`（+`type`+`currency_code`）查重，分类按 `name`（+`kind`+`parent`）查重；已存在则返回已有记录的 id，不报错、不重复插入。
- 新增 `GET /api/v1/currencies`，返回 `{code, name, symbol, decimal_places}`。HTTP 端点由 5 个增至 6 个。
- 不加导入汇总/校验端点：batch 的逐条结果（含 `duplicate`）由 AI 自行汇总即可。

### 黑洞账户（BlackHoleAccount）
- 新增 `accounts.is_hidden` 列（INTEGER，默认 0）。
- 迁移种子预置两个黑洞账户：`无(CNY)`、`无(HKD)`（`is_hidden = 1`，type 为 `other`）。不预置其余种子币种——个人记账币种有限，真遇到新币种补一条迁移即可。
- 黑洞账户由**种子保证存在**，不依赖 AI 创建；API 不需要创建 `is_hidden` 账户的能力。
- `is_hidden` 过滤范围：
  - **过滤**：用户侧 `list_accounts` / `list_accounts_with_balance` 的 WHERE、`compute_all_balances` 的 WHERE（保证余额视图/汇总不污染）、前端各账户下拉选择器（依赖 list 接口）。
  - **不过滤**：交易列表（JOIN accounts 取账户名，显示"无"）、报表聚合、`GET /api/v1/accounts`（AI 需要看到含 `is_hidden` 的完整列表）。
- `无` 交易的 kind 照常按金额正负判定为 income/expense，只是 `account_id` 指向黑洞账户；`x → 无` / `无 → x` 才按转账（`to_account_id` = 黑洞账户）处理。

### 删除旧导入路径
- 删除 `commands/import.rs`、`import_parser/mod.rs`（含其 14 个单元测试）、`ImportView.vue`、`src/api/index.ts` 的 `previewImport` 方法。
- 同步更新 `AGENTS.md` 中"导入流程"一节。

### AI 导入指导文档
- 新增一份 AI 导入指导文档（`src-tauri/prompts/` 或 `specs/` 下），约定：Pixiu 列映射（`流入金额`/`流出金额` → 正负判定 kind）、`A → B` 拆分为 `account_id`+`to_account_id`、`无`/`→ 无` 映射黑洞账户、中文币种名到 `currency_code` 的映射、金额 `*_cents` 分单位、日期格式 YYYY-MM-DD。

## Testing Decisions

### 测试策略
- **最高 seam**：HTTP API 集成测试——用 `tower::ServiceExt::oneshot` 对 axum Router 发请求 + 内存 SQLite，与现有 `src-tauri/tests/api_server.rs` 完全同构。绝大多数行为在这一层验证。
- 每个测试用例独立 Router + 独立 in-memory DB，测试间不共享状态。
- 唯一例外 seam：`dedup_hash` 纯函数单元测试，把"字段集 + 空 `to_account_id` 拼空串"两个决定钉死。
- `is_hidden` 过滤用现有 `commands/accounts.rs` 的单元测试扩展：验证 hidden 账户不进列表、但其交易仍在交易列表/报表。

### 测试内容
- 去重：同一批交易写入两次，第二次全部 `duplicate: true` 且库中行数不增。
- 去重开关：`dedup: false` 时重复写入成功（新增行）。
- 去重字段集：仅改 note/category 的同一条交易命中去重；改金额/账户/日期则不命中。
- 软删除：删除后重跑导入重新写入。
- 账户/分类幂等：同名创建两次返回同一 id，库中仅一行。
- 黑洞账户：种子存在、`is_hidden=1`、AI 的 GET accounts 可见、用户侧 IPC 不可见、其交易在交易列表中可见。
- `GET /api/v1/currencies` 返回种子币种清单。
- batch 请求体 wrapper：`{transactions, dedup}` 缺省 `dedup=true`。

### 先例参考
- 现有 HTTP 层测试：`src-tauri/tests/api_server.rs`（axum + tower + in-memory DB）。
- 现有单元测试：`commands/*.rs` 内的 `setup()` + `open_in_memory()` 模式。
- BDD：`cucumber` + in-memory DB（`src-tauri/tests/e2e/`），如需可补充场景。

## Out of Scope

- 不做"按来源/格式"的解析器框架或插件——解析是 AI 的职责。
- 不实现 `transactions.dedup_hash` 的唯一约束（全局强制去重）。
- 不为黑洞账户做专门的用户界面（不新增"待修正"视图；修正靠交易列表改挂账户完成）。
- 不预置全部种子币种的黑洞账户，仅 CNY/HKD。
- 不实现导入进度条/汇总面板。
- 多币种汇率换算仍不在 MVP 内（`amount_native_cents` 保持 1:1）。

## Further Notes

- 领域术语将同步更新 `CONTEXT.md`：新增"黑洞账户（BlackHoleAccount）"、"导入去重（ImportDedup）"、"is_hidden" 语义条目。
- 转账拆分、`无` 映射、币种名映射属于 AI 职责边界，但必须以文档化的确定性约定固化，否则 AI 每次拆法不同会让 `dedup_hash` 漂移。
- `GET /api/v1/accounts` 返回含 `is_hidden` 字段；用户侧 Tauri IPC 的 `list_accounts` 过滤 `is_hidden=1`。两个入口语义不同，需在实现时注意区分。
- 去重误吞（同一天同账户同金额的两笔独立消费）概率极小，接受并可手动修复。
