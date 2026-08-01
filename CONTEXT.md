# 领域术语表（Ubiquitous Language）

> 本文件记录 Ledger 项目中的核心领域术语。只包含业务概念，不包含实现细节。

## ScheduledTransaction（定时交易）

- **定义**：一种按照预定规则在将来多次触发生成资金变动的协议/模板。它不是交易本身，而是生成真实交易的规则。
- **边界**：
  - 每次触发时生成一条 `Transaction`（交易流水）。
  - 生成的 `Transaction` 是普通的交易记录，参与余额计算、预算统计。
  - 目前涵盖三种业务形态：分期付款（installment）、定期订阅（subscription）、定时转账（scheduled_transfer）。
  - 数据层面采用“核心表 + 扩展表”：通用字段在 `scheduled_transactions`，类型特有字段在 `installment_plans` / `subscription_plans` / `scheduled_transfer_plans`。
- **别名**：不使用“定时任务”（偏技术/调度）、“定时计划”（含糊）、“RecurringPayment”（不能涵盖转账）等词。

## InstallmentPlan（分期计划）

- **定义**：在固定期数内、按固定周期偿还一笔已知总金额的 `ScheduledTransaction`。
- **边界**：
  - 记录总金额 `total_amount_cents` 和总期数 `total_occurrences`。
  - 每期金额由总金额和总期数计算，尾差放在最后一期。
  - 已还金额和已还期数由 `scheduled_transaction_occurrences` 的 `completed` 状态实时汇总。
  - 每次触发时生成一条 `Transaction`（`kind = expense`）。
- **别名**：不使用“loan”、“debt”等词，因为分期不一定是负债（例如分期购买服务）。

## Subscription（订阅）

- **定义**：按周期持续扣款，直到用户手动取消或暂停的 `ScheduledTransaction`。
- **边界**：
  - MVP 阶段没有结束日期，也没有最大期数限制。
  - 只能通过 `paused` 或 `cancelled` 状态停止。
  - 金额固定，MVP 不支持中途涨价。
  - 每次触发时生成一条 `Transaction`（`kind = expense`）。
- **别名**：不使用“membership”、“recurring payment”等词，除非业务明确需要区分。

## ScheduledTransfer（定时转账）

- **定义**：在预定日期从用户一个账户向另一个账户转出固定金额的 `ScheduledTransaction`。
- **边界**：
  - 必须指定转出账户和转入账户。
  - 可以是一次性（只执行一期）或周期性（循环执行）。
  - 每次触发时生成一条 `Transaction`（`kind = transfer`）。
- **别名**：不使用“auto transfer”、“standing order”等银行术语，除非业务明确需要。

## Occurrence（期次）

- **定义**：`ScheduledTransaction` 的一次应执行实例。每期对应一个触发日期，可能生成一条 `Transaction`。
- **边界**：
  - 已发生的期次必须实例化落库，记录执行状态和生成的交易 ID。
  - 未来期次只预生成有限窗口（如未来 N 期或 N 个月），远期按需展开。
  - 单期可独立查看、重试，不破坏整个计划。
  - 数据层面统一使用 `scheduled_transaction_occurrences` 表。
- **状态**：`pending`（待执行）、`processing`（执行中）、`completed`（已完成）、`failed`（失败）、`skipped`（已跳过）、`cancelled`（已取消）。
- **别名**：不使用“任务实例”、“执行记录”等偏技术词汇。

## Transaction（交易流水）

- 见现有 schema 定义：一笔实际发生的资金变动，已存在于 V001。
- 在定时交易语境下，它是 `ScheduledTransaction` 的某期执行产物。

## Plan Lifecycle（计划生命周期）

- **MVP 决策**：`ScheduledTransaction` 支持以下状态变更：
  - `active`（正常执行）
  - `paused`（暂停，不再生成新期次）
  - `cancelled`（取消，所有未执行期次状态变为 `cancelled`）
  - `completed`（计划自然完成，所有期次已执行）
- **MVP 不支持**：单独取消/跳过某期、修改单期金额或日期。
- **边界**：
  - 取消整个计划不会删除已生成的 `Transaction`。
  - 暂停/恢复不改变已生成的期次或交易。

## Timing（时间精度）

- **日期精度**：所有定时交易只精确到日期，不记录具体执行时间。
- **执行日期**：`Occurrence` 的 `scheduled_date` 为 ISO 8601 日期格式（YYYY-MM-DD）。
- **节假日处理**：MVP 采用严格日期，不因为周末/节假日顺延。
- **边界**：`Transaction.date` 直接复用 `Occurrence` 的 `scheduled_date`，两者保持一致。

## Counterparty（交易对手）

- **定义**：定时交易中的收款方或付款对象，例如商家、贷款机构、订阅服务商。
- **MVP 决策**：在 `InstallmentPlan` 和 `Subscription` 的扩展表中记录 `counterparty` 字段；生成 `Transaction` 时复制到 `Transaction.note` 或作为展示字段。
- **MVP 不扩展**：不在 `Transaction` 表中新增通用 `counterparty` 字段，避免改动现有核心表。
- **边界**：`ScheduledTransfer` 不使用 `counterparty`，而是使用 `to_account_id` 表示本方账户间转账。

## Amount Model（金额模型）

- **MVP 决策**：每期金额固定，使用 `ScheduledTransaction` 的 `amount_cents` 字段。
- **分期金额计算规则**：
  1. `InstallmentPlan` 记录 `total_amount_cents` 和 `total_occurrences`。
  2. 每期基准金额 = `floor(total_amount_cents / total_occurrences)`。
  3. 剩余尾差 = `total_amount_cents - base_amount_cents * total_occurrences`。
  4. 最后一期金额 = `base_amount_cents + 剩余尾差`。
  5. 其余每期金额 = `base_amount_cents`。
- **边界**：MVP 不支持每期金额不同；不支持 subscription 中途涨价。

## Recurrence Rule（周期规则）

- **MVP 决策**：使用显式字段表达周期，不引入 RRULE 等通用表达式。
- **字段**：
  - `recurrence_type`：周期类型，如 `daily`、`weekly`、`monthly`、`yearly`。
  - `recurrence_interval`：间隔，如每 1 个月、每 2 周。
  - `recurrence_day`：具体日期/星期，如每月 1 日、每周一。
- **边界**：MVP 只支持常见固定周期；复杂规则（如“每月最后一个工作日”）留到后续版本。

## Failure Policy（失败策略）

- **MVP 决策**：MVP 阶段只支持“失败即标记为 failed，由用户手动重试”。不自动重试、不自动跳过、不产生滞纳金。
- **理由**：离线优先场景下，自动重试容易在多设备间产生重复执行；手动重试让用户明确控制资金流出，适合个人账本。

## Transaction Kind Mapping（交易类型映射）

- **MVP 决策**：由 `ScheduledTransaction.kind` 固定生成对应 `Transaction.kind`，用户不可配置。
- **映射规则**：
  - `installment` → `expense`
  - `subscription` → `expense`
  - `scheduled_transfer` → `transfer`

## Category（分类）

- **定义**：对交易（Transaction）进行归类的标签体系。两级结构：顶级分类和二级子分类，不支持三级及以上嵌套。
- **边界**：
  - 每个分类属于 `income`（收入）或 `expense`（支出）两种类型之一，子分类必须与父分类类型一致。
  - 分类具有 visual identity：`icon`（图标/emoji）和 `color`（颜色），用于交易表单、报表、图表等场景的视觉辨识。
  - 分类排序由 `sort_order` 字段控制，同级内可拖拽重排，parent 变更通过编辑操作进行。
  - 软删除（`is_deleted`），删除操作同时级联删除子分类。
- **别名**：不使用 "标签"、"类别"、"分组" 等词，统一使用 "分类"。

## DefaultCurrency（默认币种）

- **定义**：用户在记账时首选的基准币种，用于展示本位币金额（`amount_native_cents`）。
- **边界**：MVP 阶段所有币种与默认币种汇率默认为 1:1；默认币种的选择影响 `formatAmount` 的 `decimal_places` 行为和报表的显示货币。
- **别名**：不使用"本位币"（偏会计术语）、"主币"（偏支付）。

## Appearance（外观）

- **定义**：用户对应用视觉呈现的选择，当前支持暗色（`dark`）和亮色（`light`）两种主题。
- **边界**：主题切换仅影响视觉表现层，不改变业务逻辑；MVP 阶段默认为暗色主题。
- **别名**：不使用"皮肤"（偏自定义程度更高的概念）、"配色方案"。

## AI API（AI 编程接口）

- **定义**：Ledger 在 `127.0.0.1:9527` 上提供的 RESTful HTTP API，专供 AI 编程助手（如 Cursor、Claude Code）通过 HTTP 请求读写 Ledger 数据。
- **边界**：
  - 仅监听 localhost，无认证，适用于单机桌面场景。
  - URL 前缀 `/api/v1`，JSON 请求/响应。
  - 错误格式复用 `{kind, message}`。
  - **专用场景**：数据迁移（从第三方 APP 的 CSV/Excel 导入）。
  - **暴露的接口**：`accounts`（list/create）、`categories`（list/create）、`transactions/batch`、`currencies`（list）、`import/knowledge`，共 7 个端点。
  - `accounts` / `categories` 的 create 按自然键幂等（同名复用已有记录）；`transactions/batch` 支持 `dedup` 参数（默认开启）。
  - `import/knowledge` 返回精简的导入约定文本（Pixiu 列映射、转账拆分、黑洞账户、币种映射、分单位、日期、dedup），供 AI 直接注入系统提示词。
- **别名**：不使用"本地 API"（过于泛化）、"后端 API"（与 Tauri IPC 混淆）。

## ImportDedup（导入去重）

- **定义**：在 `POST /api/v1/transactions/batch` 导入入口，由后端对每条 `TransactionInput` 计算确定性内容哈希（`dedup_hash`），命中已存在的未删除交易则跳过并返回 `duplicate`，避免重复导入污染账本。
- **哈希**：`sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)`，`to_account_id` 缺省空串。字段集排除 note/category（AI 生成文本非确定性，会导致哈希漂移）。
- **边界**：
  - 只在导入入口生效，手工记账与定时交易引擎不受影响；`dedup` 参数默认开启、可关闭。
  - 只匹配 `is_deleted=0` 的交易：软删除的交易不占去重位，重跑导入会重新写入。
  - `dedup_hash` 导入后保持不变，编辑/同步无特殊处理。
  - `dedup_hash` 落库但不建唯一约束——去重是应用层行为，不是数据库硬约束。

## BlackHoleAccount（黑洞账户）

- **定义**：用于承接来源不明资金变动的占位账户（如第三方导出中 `资金账户=无` 的交易），作为数据修正的缓冲池。交易照常写入、参与列表与报表，但账户本身对用户隐藏。
- **边界**：
  - 是 `accounts` 表中的真实记录，`is_hidden=1`；按币种预置（当前为 `无(CNY)`、`无(HKD)`），由迁移种子保证存在，不依赖导入方创建。
  - `is_hidden` 只过滤账户的展示与余额汇总（账户列表、下拉选择器、`compute_all_balances`）；其交易仍正常出现在交易列表与报表，便于用户改挂到真实账户后清空删除。
  - 对 AI 的 `GET /api/v1/accounts` 可见（返回 `is_hidden` 标志），以便把"无"交易映射到黑洞账户。
  - `无` 交易的 kind 照常按金额正负判定为 income/expense；`x → 无` / `无 → x` 按转账处理（`to_account_id` 指向黑洞账户）。
- **别名**：不使用"垃圾账户"、"清理账户"（偏贬义/一次性）；不使用"未知账户"（与未知金额混淆）。

## Instrument（金融工具/标的）

- **定义**：用户可交易的金融产品，例如某只股票、某只基金、某只债券。
- **边界**：
  - 包含五种类型：`stock`（股票）、`fund`（基金）、`bond`（债券）、`etf`（ETF）、`other`（其他）。
  - `symbol` 为显示代码（如 `600519`、`000001`、`00700`），`market` 区分市场（`sh`/`sz`/`hk`；手动创建未指定时为 `unknown`），两者共同标识一个标的。
  - 股票类标的由"标的全量同步"从东方财富 API 批量拉取填充（代码、名称、市场、币种），用户无需手动录入；基金、债券等非股票标的仍可手动创建。
  - `name` 为标的名称（如 "贵州茅台"），同步时自动填充。
- **别名**：不使用"证券"（偏法律概念）、"产品"（过于泛化）。不单独使用 "market_symbol" 或 "ticker"——统一使用 "symbol"。

## MarketPrice（市场价格）

- **定义**：某个 Instrument 在某个时间点的市场报价快照。
- **边界**：
  - MVP 阶段不保留历史价格：每个 Instrument 全局只保留一条 MarketPrice 记录，每次同步覆盖更新。
  - `price_cents` 和 `currency_code` 由数据源（东方财富）返回，币种按市场固定：`sh`/`sz` → `CNY`，`hk` → `HKD`。
  - `priced_at` 记录同步时间。
  - `source` 标记数据来源，当前为 `eastmoney`。
- **别名**：不使用"行情"（偏实时交易概念）、"报价"（偏询价场景）。

## Holding（持仓）

- **定义**：用户在某个投资账户中持有的某个 Instrument 的数量及成本信息。
- **边界**：
  - `quantity` 为当前持有数量（由各持仓批次 `remaining_quantity` 实时聚合）。
  - `cost_basis_cents` 为持有成本（买入总金额），`cost_currency_code` 为成本币种。
  - `latest_price_cents`、`market_value_cents`、`unrealized_pnl_cents` 由 `v_holdings` 视图实时计算（关联最新 MarketPrice 与汇率折算到账户本位币），不落库存储。
  - 市值 = quantity × latest_price_cents（按汇率折算）；未实现盈亏 = 账户币市值 - 账户币成本。
- **别名**：不使用"仓位"（偏交易术语）、"库存"（偏实物）。

## InstrumentSync（标的全量同步）

- **定义**：用户手动触发、系统从东方财富 API 一次性全量拉取沪市/深市/港股股票标的信息（代码、名称、最新价），upsert 到本地 `instruments` 与 `market_prices` 的过程。
- **边界**：
  - 触发方式：手动触发（设置页"股票标的全量同步"），不做自动定时刷新。
  - 同步范围：沪市（`sh`）、深市（`sz`）、港股（`hk`），币种分别固定为 `CNY`/`CNY`/`HKD`。
  - 执行方式：一次性全量同步、分页拉取，无并发限制、无失败重试。
  - 去重：按 `symbol` 匹配已有标的，名称或市场变更则更新，不存在则插入，并同步 upsert 该标的最新价到 `market_prices`。
  - 进度反馈：通过 `sync-instruments:progress` 事件向前端汇报当前页数/总数与累计新增、更新数量。
- **别名**：不使用"同步价格"（偏持续同步）、"更新行情"（偏实时）。
