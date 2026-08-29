# 领域词汇表：AI 导入

> Ledger 领域词汇表的 AI 导入分域。全部分域与彼此关系见 `CONTEXT-MAP.md`；决策记录见 `docs/adr/`（ADR-0010 等）。
> 跨域共享术语（Transaction、Transaction Kind Mapping、InvolvingAccount、参考数据等）见核心交易域 `CONTEXT-core.md` / 参考数据与设置域 `CONTEXT-reference-settings.md`，本文不复制定义。
> 若与代码行为冲突，以代码为准并同步修正本文件。

## AI API（AI 编程接口）

- **定义**：Ledger 在 `127.0.0.1:9527` 上提供的 RESTful HTTP API，专供 AI 编程助手（如 Cursor、Claude Code）通过 HTTP 请求读写 Ledger 数据。
- **边界**：
  - 仅监听 localhost，无认证，适用于单机桌面场景。
  - URL 前缀 `/api/v1`，JSON 请求/响应。
  - 错误格式复用 `{kind, message}`。
  - **场景**：主要场景是数据迁移（从第三方 APP 的 CSV/Excel 导入），亦可直接录入记账（账户/分类幂等创建、交易携带商户名字符串、批量写核心交易域 Transaction）；迁移完成后支持读回验证与纠错（删除/修改，见 AIReadbackVerification / AICleanupDeletion / AICleanupModify）。
  - **暴露的接口**（13 个端点）：`openapi.json`、`accounts`（list/create/update/delete）、`accounts/balances`（含黑洞账户）、`categories`（list/create/delete）、`transactions`（list，可按日期/转出账户/涉及账户/商户/类型过滤）、`transactions/batch`、`transactions/{id}`（delete/update）、`currencies`（list）、`merchants`（list，在用商户字典）、`import/knowledge`。
  - `accounts` / `categories` 的 create 按自然键幂等（同名复用已有记录）；`transactions/batch` 支持 `dedup` 参数（默认开启）与客户端 `idempotency_key`（见 ImportDedup / IdempotencyKey）；交易行可带 `merchant_name`（商户名字符串，与 `merchant_id` 互斥）：后端写入路径精确匹配在用商户名，命中复用、未命中即建——商户归一化责任收口在后端，AI 不负责商户去重，幂等重放不产生碎商户（issue #194 / ADR-0028）。
  - `import/knowledge` 返回精简的导入约定文本（Pixiu 列映射、转账拆分、黑洞账户、币种映射、商户约定、分单位、日期、dedup），供 AI 直接注入系统提示词。
- **别名**：不使用"本地 API"（过于泛化）、"后端 API"（与 Tauri IPC 混淆）。

## AIReadbackVerification（AI 读回验证）

- **定义**：AI 编程助手完成批量导入后，通过读回接口核对迁移结果是否完整的环节——用 `GET /api/v1/transactions` 按日期区间/转出账户/涉及账户/类型过滤读回核心交易域 Transaction，核对源文件各行是否全部落库、金额合计是否一致；再用 `GET /api/v1/accounts/balances` 拿到各账户（**含黑洞账户**）实时余额，核对期末余额与源数据吻合。
- **边界**：
  - 读回是查询能力：`transactions` 返回未删除交易（按 `date DESC` 排序），`balances` 口径 = 初始余额 + Σ `account_flow`（各 kind 符号归属见核心交易域 Transaction Kind Mapping，含投资类），实时计算不持久化。
  - 对账要点：余额清单包含黑洞账户，可识别误挂到 `无` 的交易；转账按转出账户对账，需核对转入侧时改用涉及账户过滤读回（`involving_account_id`，`account_id` 或 `to_account_id` 命中即算，见核心交易域 InvolvingAccount）。
  - 与手工记账共用同一套查询实现，无独立数据视图。
- **别名**：不使用"审计"（偏外部合规）、"校验导入"（含糊）。

## AICleanupDeletion（AI 纠错删除）

- **定义**：AI 编程助手读回发现写错的数据后，通过软删除接口纠正的环节——`DELETE /api/v1/transactions/{id}` 删除错行，`DELETE /api/v1/accounts/{id}`、`DELETE /api/v1/categories/{id}` 删除误建记录，删除后重跑同一批导入即可重新写回。
- **边界**：
  - 全部软删除（`is_deleted=1`），与 UI 删除行为一致（IPC 与 HTTP 共用同一内部函数）；核心交易域 buy 交易删除同步清理投资域关联持仓。
  - 删除后重跑导入可重新写入：去重只匹配 `is_deleted=0` 的交易，软删除不占去重位，同一份源文件可反复安全重跑。
  - 删除不校验引用（与 UI 一致）：删除有交易的账户后历史交易仍保留，由用户/AI 自行管理。
  - 不存在的 id 返回 404。
- **别名**：不使用"回滚"（偏事务语义）、"清理"（偏一次性）。

## AICleanupModify（AI 纠错修改）

- **定义**：AI 编程助手读回发现写错的核心交易域 Transaction 后，用修改接口按 `id` 纠错、而非"删除→重导"的环节——`PUT /api/v1/transactions/{id}` 全字段替换该交易，幂等键保持不变。
- **边界**：
  - 与 AICleanupDeletion 互补：删账户/删分类/整笔移除仍走软删除；单笔交易写错用"改"而非"删后重导"，避免重导覆盖界面手动编辑、也不产生重复。
  - 编辑不重算去重身份：幂等键不变；内容哈希兜底行被编辑后 `dedup_hash` 不再准确，仅影响旧兜底路径，新导入（带幂等键）不受影响。
- **别名**：不使用"更新"/"PUT"（偏实现细节）、"纠错覆盖"（含糊）。

## ImportDedup（导入去重）

- **定义**：在 `POST /api/v1/transactions/batch` 导入入口，由后端判断"这条核心交易域 Transaction 是否已导入过"并跳过、返回 `duplicate`，避免重复导入污染账本。
- **幂等键优先**：每条 `TransactionInput` 可带客户端提供的 `idempotency_key`（见 IdempotencyKey）。带键时，去重以幂等键为准——命中已存在的未删除交易则跳过，与内容无关。
- **内容哈希兜底**：不带幂等键的行，回退到确定性内容哈希 `dedup_hash = sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)`（`to_account_id` 缺省空串，排除 note/category）。这是冻结契约的保留路径，仅作旧调用兜底。
- **边界**：
  - 只在导入入口生效，手工记账与定时计划域定时交易引擎不受影响；`dedup` 参数默认开启、可关闭。
  - 只匹配 `is_deleted=0` 的交易：软删除的交易不占去重位，重跑导入会重新写入。
  - 目标：新导入一律带幂等键，让内容哈希退化为历史兼容路径，避免 buy/sell 等"雷同交易"被内容哈希误去重。

## IdempotencyKey（导入幂等键）

- **定义**：客户端在批量导入时给每条 `TransactionInput` 提供的、内容无关的稳定标识，指向"这条交易来自源文件的哪一行"。
- **边界**：
  - 客户端自行保证唯一；服务端只约束"同键至多对应一笔未删除交易"（部分唯一索引 `WHERE idempotency_key IS NOT NULL AND is_deleted=0`），并据此做到 O(log n) 去重查询。
  - 内容无关：编辑交易内容（金额/账户/备注等）不改变幂等键，故编辑不导致去重身份漂移——这是相较内容哈希的核心优势。
  - 不可编辑：修改 API 不提供改幂等键的入口；幂等键只在导入时落定。
  - 一次源行拆多笔时，客户端派生"源文件:行号:交易序号"的独立键。
- **别名**：不使用"去重哈希"（偏内容签名）、"duplicate key"（偏数据库约束）。

## BlackHoleAccount（黑洞账户）

- **定义**：用于承接来源不明资金变动的占位账户（如第三方导出中 `资金账户=无` 的核心交易域 Transaction），作为数据修正的缓冲池。交易照常写入、参与列表与报表，但账户本身对用户隐藏。
- **边界**：
  - 是参考数据 `accounts` 表中的真实记录，`is_hidden=1`；按币种预置（当前为 `无(CNY)`、`无(HKD)`），由迁移种子保证存在，不依赖导入方创建；此外参考数据与设置域 BalanceAdjustment 会按需自动创建缺失币种的黑洞账户（运行时 `ensure`，与种子同形：`无(XXX)`、type=`other`、`is_hidden=1`）。
  - `is_hidden` 只过滤账户的展示与余额汇总（账户列表、下拉选择器、`compute_all_balances`）；其交易仍正常出现在交易列表与报表，便于用户改挂到真实账户后清空删除。
  - 对 AI 的 `GET /api/v1/accounts` 可见（返回 `is_hidden` 标志），以便把"无"交易映射到黑洞账户。
  - `无` 交易的 kind 照常按金额正负判定为 income/expense；`x → 无` / `无 → x` 按转账处理（`to_account_id` 指向黑洞账户）。
- **别名**：不使用"垃圾账户"、"清理账户"（偏贬义/一次性）；不使用"未知账户"（与未知金额混淆）。

## AIPrompt（AI 提示词）

- **定义**：人类用户在“AI”菜单中查看、可一键复制给 AI 编程助手（如 Cursor、Claude Code）的系统提示词文本。AI 收到后据此通过本地 HTTP API 读写账本：先发现端点，再获取导入知识，完成迁移后读回对账，必要时纠错（删除/修改）。
- **边界**：
  - 与导入知识（import knowledge）互补：提示词是入口指引，导入知识是拆行约定细节；AI 按提示词指引自行通过 HTTP 获取导入知识，人类用户无需复制后者。
  - 与 AI API（AI 编程接口）的关系：AI API 是 AI 主动调用的接口，AI 提示词是人类主动提供给 AI 的入口文本，二者共同构成 AI 导入闭环。
- **别名**：不使用“系统提示词”（偏通用 AI 术语，未指明 Ledger 场景）。
