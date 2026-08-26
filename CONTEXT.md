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

- **定义**：一笔实际发生的资金变动，已存在于 V001；在定时交易语境下，它是 `ScheduledTransaction` 的某期执行产物。
- **kind（交易类型，8 种，真源为 `transaction::amount::TransactionKind` 枚举）**：
  - `income`（收入）/ `expense`（支出）：日常收支。
  - `transfer`（转账）：`account_id` 转出、`to_account_id` 转入。
  - `refund`（退款）：关联原支出交易（`refund_of_transaction_id`），账户/币种/分类继承原支出。
  - `buy`（买入证券）/ `sell`（卖出证券）：资本变动，关联投资持仓（见 Instrument / Holding）。
  - `dividend`（现金分红）：计入收入。
  - `split`（拆股/送股）：现金影响恒为 0。
  - **每种 kind 对各金额度量的符号归属见 Transaction Kind Mapping；交易行写入与校验收口在 `transaction::writer`（Writer 接缝）。**
  - **kind 为闭集（8 种），行为收敛分派在行为层**：每类 kind 的校验/归一化/应用副作用/回退经 `commands::transactions::behavior`（`plan → apply / revert`）单点分派——通用 kind 走 Writer 接缝，buy/sell 委托投资域（`commands::investment` 的 prepare/apply/revert），不再散落多处 `match kind`（issue #72）。
  - **`dividend` / `split` 已声明但未实现（MVP）**：经交易接口（创建/修改）显式返回「暂不支持」错误，不落库、不静默误记。
- **列表呈现边界**：
  - 交易列表按 `date` 倒序呈现（最新在前）。
  - 采用服务端 offset 分页：一次只取当前页交易，并返回满足筛选条件的交易总数（"共 N 条"）。
  - 翻页浏览期间新增交易会使列表整体前移，后续页可能看到条目重复或遗漏——这是已知行为，不是数据错误；重新进入列表即恢复一致。
  - 筛选条件变化时总数随之变化，分页始终基于筛选后的结果。

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

**raw/native 分离（`transactions` 行级）**：
- `amount_cents`：原始币种金额；`amount_native_cents`：本位币金额（折算到全局默认币种 DefaultCurrency）。
- 折算由 `transaction::amount::convert_to_native` 统一执行：与默认币种相同 → 1:1；否则按汇率折算（正反向汇率兜底），缺汇率报错、不静默混币种。MVP 阶段多币种汇率 1:1，故二者恒等。
- 折算基准为全局默认币种、与账户币种无关，避免跨账户汇总口径漂移。
- 四个具名度量（`account_flow` / `expense_net` / `income_net` / `refund_gross`）对 8 种 kind 的符号归属见 Transaction Kind Mapping。

**分期金额计算（ScheduledTransaction）**：
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

**ScheduledTransaction → Transaction 生成映射（用户不可配置）**：
- `installment` → `expense`
- `subscription` → `expense`
- `scheduled_transfer` → `transfer`

**Transaction.kind → 金额度量归属矩阵**（8 种 kind，真源为 `transaction::amount` 的系数矩阵，同时驱动 SQL 聚合片段与行级 `signed_amount`）：

| kind | account_flow | expense_net | income_net | refund_gross |
|------|------|------|------|------|
| income | + | 0 | + | 0 |
| expense | − | + | 0 | 0 |
| transfer | account_id=− / to_account_id=+ | 0 | 0 | 0 |
| refund | + | − | 0 | + |
| buy | − | 0 | 0 | 0 |
| sell | + | 0 | 0 | 0 |
| dividend | + | 0 | + | 0 |
| split | 0 | 0 | 0 | 0 |

- `account_flow`（账户现金流动）：余额口径，某账户视角的现金出入；transfer 按侧取号。
- `expense_net`（支出净额）= 毛支出 − 退款；buy/sell 属资本变动，不计入经营收支。
- `income_net`（收入净额）= 收入 + 分红。
- `refund_gross`（退款毛额）：独立成列，毛值/净值并存展示。
- 净值恒等式（一处定义）：`expense_net = expense_gross − refund_gross`，月度汇总毛值由此导出。

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

## Reference Data（参考数据）

- **定义**：`currencies / accounts / categories` 三张参考表及其全部派生映射（币种映射、账户映射、分类映射、分类树）的统称，作为 UI 字典 / 枚举的**单一来源**，描述"有哪些可选值"。与交易流水（`Transaction`，描述"发生了什么"）相对：参考数据量小、变化低频、常驻内存；交易量大、持续追加、按需分页。
- **边界**：
  - 前端单一来源是 `useReferenceStore`（Pinia store），持有三张参考表与全部派生映射 / 分类树逻辑（`currencyMap / accountMap / categoryMap / rootCategories / expenseCategories / incomeCategories / categoryChildren / categoryPath / treeCategoryOptions`）与失效信号（`status / version`）；`useAppStore` 已收缩为纯 UI 设置 store（主题 / 默认币种 / 备份设置），不再暴露参考数据接口（issue #85）。
  - 被 `Transaction` 以外键引用（账户 / 分类 / 币种）；参考数据改名、删除、新建会级联反映到所有消费它的界面（交易列表与报表里的名称、表单下拉选项、分类树、预算的分类聚合）。
  - **运行期可被外部修改**：AI 编程助手经本地 HTTP API（见 AI API）导入 / 修改参考数据，发生在 Tauri 应用之外；应用自身的账户 / 分类管理同样修改参考数据。
  - **失效信号（ADR-0012）**：任一参考写入成功后，后端发出通用、粗粒度、无 payload 的 `ledger:changed` 信号（已落地，见 issue #79）；前端订阅该信号自动重拉三张参考表，属 ADR-0012 设计目标、随 spec #76 各子任务落地中。交易类写入不触发（不改参考表）。
  - **重拉语义（stale-while-revalidate，ADR-0012 设计目标）**：保留旧数据，成功后才整体替换，界面不闪空；`status`（`idle | loading | ready | error`）与 `version`（每次成功重拉自增）暴露新鲜度，消费者可显式感知数据是新鲜还是过期。
  - 前端只持**内存态缓存**：参考数据的真源是 SQLite 数据库，前端 store 不持久化副本，也不随 Backup / Restore 迁移（那些是文件级快照与设备本地偏好，见 Backup / ViewState）。
- **别名**：不使用"账本快照（ledger snapshot）"——那会误导读者以为缓存了全部账本数据（见 Backup / Restore 的文件级快照）；不使用"基础数据"（过于泛化，易与业务字段混淆）。

## Appearance（外观）

- **定义**：用户对应用视觉呈现的选择，当前支持暗色（`dark`）和亮色（`light`）两种主题。
- **边界**：主题切换仅影响视觉表现层，不改变业务逻辑；MVP 阶段默认为暗色主题。**强调色（品牌色）与语义色（业务色）相互独立**：强调色为琥珀暖橙，跨暗/亮主题一致（亮色用同色相加深版以保证按钮白字对比度），标识交互与选中态；语义色（收入绿/支出红/退款蓝）表达金额的业务含义，硬编码于列表与图表，不随主题变化。
- **别名**：不使用"皮肤"（偏自定义程度更高的概念）、"配色方案"。

## AI API（AI 编程接口）

- **定义**：Ledger 在 `127.0.0.1:9527` 上提供的 RESTful HTTP API，专供 AI 编程助手（如 Cursor、Claude Code）通过 HTTP 请求读写 Ledger 数据。
- **边界**：
  - 仅监听 localhost，无认证，适用于单机桌面场景。
  - URL 前缀 `/api/v1`，JSON 请求/响应。
  - 错误格式复用 `{kind, message}`。
  - **场景**：主要场景是数据迁移（从第三方 APP 的 CSV/Excel 导入），亦可直接录入记账（账户/分类幂等创建、批量写交易）；迁移完成后支持读回验证与纠错（删除/修改，见 AIReadbackVerification / AICleanupDeletion / AICleanupModify）。
  - **暴露的接口**（13 个端点）：`openapi.json`、`accounts`（list/create/delete）、`accounts/balances`（含黑洞账户）、`categories`（list/create/delete）、`transactions`（list，可按日期/账户/类型过滤）、`transactions/batch`、`transactions/{id}`（delete/update）、`currencies`（list）、`import/knowledge`。
  - `accounts` / `categories` 的 create 按自然键幂等（同名复用已有记录）；`transactions/batch` 支持 `dedup` 参数（默认开启）与客户端 `idempotency_key`（见 ImportDedup / IdempotencyKey）。
  - `import/knowledge` 返回精简的导入约定文本（Pixiu 列映射、转账拆分、黑洞账户、币种映射、分单位、日期、dedup），供 AI 直接注入系统提示词。
- **别名**：不使用"本地 API"（过于泛化）、"后端 API"（与 Tauri IPC 混淆）。

## AIReadbackVerification（AI 读回验证）

- **定义**：AI 编程助手完成批量导入后，通过读回接口核对迁移结果是否完整的环节——用 `GET /api/v1/transactions` 按日期区间/账户/类型过滤读回交易，核对源文件各行是否全部落库、金额合计是否一致；再用 `GET /api/v1/accounts/balances` 拿到各账户（**含黑洞账户**）实时余额，核对期末余额与源数据吻合。
- **边界**：
  - 读回是查询能力：`transactions` 返回未删除交易（按 `date DESC` 排序），`balances` 口径 = 初始余额 + 收入 − 支出 + 转入 − 转出 + 退款，实时计算不持久化。
  - 对账要点：余额清单包含黑洞账户，可识别误挂到 `无` 的交易；转账按转出账户对账（MVP 不按转入账户过滤）。
  - 与手工记账共用同一套查询实现，无独立数据视图。
- **别名**：不使用"审计"（偏外部合规）、"校验导入"（含糊）。

## AICleanupDeletion（AI 纠错删除）

- **定义**：AI 编程助手读回发现写错的数据后，通过软删除接口纠正的环节——`DELETE /api/v1/transactions/{id}` 删除错行，`DELETE /api/v1/accounts/{id}`、`DELETE /api/v1/categories/{id}` 删除误建记录，删除后重跑同一批导入即可重新写回。
- **边界**：
  - 全部软删除（`is_deleted=1`），与 UI 删除行为一致（IPC 与 HTTP 共用同一内部函数）；buy 交易删除同步清理关联持仓。
  - 删除后重跑导入可重新写入：去重只匹配 `is_deleted=0` 的交易，软删除不占去重位，同一份源文件可反复安全重跑。
  - 删除不校验引用（与 UI 一致）：删除有交易的账户后历史交易仍保留，由用户/AI 自行管理。
  - 不存在的 id 返回 404。
- **别名**：不使用"回滚"（偏事务语义）、"清理"（偏一次性）。

## AICleanupModify（AI 纠错修改）

- **定义**：AI 编程助手读回发现写错的交易后，用修改接口按 `id` 纠错、而非"删除→重导"的环节——`PUT /api/v1/transactions/{id}` 全字段替换该交易，幂等键保持不变。
- **边界**：
  - 与 AICleanupDeletion 互补：删账户/删分类/整笔移除仍走软删除；单笔交易写错用"改"而非"删后重导"，避免重导覆盖界面手动编辑、也不产生重复。
  - 编辑不重算去重身份：幂等键不变；内容哈希兜底行被编辑后 `dedup_hash` 不再准确，仅影响旧兜底路径，新导入（带幂等键）不受影响。
- **别名**：不使用"更新"/"PUT"（偏实现细节）、"纠错覆盖"（含糊）。

## ImportDedup（导入去重）

- **定义**：在 `POST /api/v1/transactions/batch` 导入入口，由后端判断"这条交易是否已导入过"并跳过、返回 `duplicate`，避免重复导入污染账本。
- **幂等键优先**：每条 `TransactionInput` 可带客户端提供的 `idempotency_key`（见 IdempotencyKey）。带键时，去重以幂等键为准——命中已存在的未删除交易则跳过，与内容无关。
- **内容哈希兜底**：不带幂等键的行，回退到确定性内容哈希 `dedup_hash = sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)`（`to_account_id` 缺省空串，排除 note/category）。这是冻结契约的保留路径，仅作旧调用兜底。
- **边界**：
  - 只在导入入口生效，手工记账与定时交易引擎不受影响；`dedup` 参数默认开启、可关闭。
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

- **定义**：用于承接来源不明资金变动的占位账户（如第三方导出中 `资金账户=无` 的交易），作为数据修正的缓冲池。交易照常写入、参与列表与报表，但账户本身对用户隐藏。
- **边界**：
  - 是 `accounts` 表中的真实记录，`is_hidden=1`；按币种预置（当前为 `无(CNY)`、`无(HKD)`），由迁移种子保证存在，不依赖导入方创建。
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

## Investment（投资域）

- **定义**：承载证券交易（buy/sell）背后持仓批次、卖出匹配与已实现盈亏的概念域，物理实现为 `commands::investment` 模块（lot / 匹配 / pnl 数据逻辑）。
- **边界**：
  - **buy/sell 首先是交易 kind**：一笔 buy/sell 先是一笔 `Transaction`（交易行落库经 Writer 接缝），Investment 是它背后的持仓/盈亏载体。
  - 对外出口收窄为 `prepare / apply / revert` 三件套（issue #72）：prepare 校验归一化（不落库）、apply 应用副作用（buy 建仓 / sell 卖出匹配）、revert 回退副作用（buy 守卫+清理 / sell 回补）；交易行写入由交易行为层编排（经 `transaction::writer`），Investment 不再反向依赖 transactions 的行更新函数（双向依赖已斩断，issue #70）。
  - 分派用薄而穷尽的 `match`，不引入 trait 注册表（避免过度设计）。
- **别名**：不使用“投资账户”（那是 `AccountType::Investment` 账户）、“证券模块”（偏数据层）。

## InstrumentSync（标的全量同步）

- **定义**：用户手动触发、系统从东方财富 API 一次性全量拉取沪市/深市/港股股票标的信息（代码、名称、最新价），upsert 到本地 `instruments` 与 `market_prices` 的过程。
- **边界**：
  - 触发方式：手动触发（设置页"股票标的全量同步"），不做自动定时刷新。
  - 同步范围：沪市（`sh`）、深市（`sz`）、港股（`hk`），币种分别固定为 `CNY`/`CNY`/`HKD`。
  - 执行方式：一次性全量同步、分页拉取，无并发限制、无失败重试。
  - 去重：按 `symbol` 匹配已有标的，名称或市场变更则更新，不存在则插入，并同步 upsert 该标的最新价到 `market_prices`。
  - 进度反馈：通过 `sync-instruments:progress` 事件向前端汇报当前页数/总数与累计新增、更新数量。
- **别名**：不使用"同步价格"（偏持续同步）、"更新行情"（偏实时）。

## WindowState（窗口状态）

- **定义**：应用窗口的几何形态——大小、位置、最大化/还原——跨启动保持的界面状态。
- **边界**：
  - 属于**界面状态**（UI State），不是业务概念；与业务"状态"（Occurrence 状态、Plan 生命周期）是两类东西，术语上必须区分。
  - 由官方窗口状态插件持久化，应用启动时自动恢复；dev 与 release 行为一致。
- **别名**：不使用"窗口大小"（只是几何的一部分）。

## ViewState（视图状态）

- **定义**：用户在界面中的位置与外观选择——当前所在视图、侧边栏折叠、报表汇总层级——跨启动保持。
- **边界**：
  - 与 WindowState 同属界面状态，与业务"状态"（Occurrence 状态、Plan 生命周期）区分。
  - MVP 仅覆盖三样：上次视图（路由）、侧边栏折叠、报表层级切换；不做"过度记忆"（不持久化筛选条件、滚动位置、列宽等）。
  - **分域存储**：界面状态（含 WindowState、ViewState、偏好）不进入 SQLite，与业务数据分库/分域；业务数据仍独占 `ledger.db`。
- **别名**：不使用"会话"（偏网络会话）、"记忆"（含糊）。

## Backup（备份）

- **定义**：账本数据库的完整文件级快照，由用户手动触发并自行选择存放位置。产物为包含数据库文件与元数据（备份时间、应用版本、schema 版本）的 zip 包。
- **边界**：
  - 是文件级快照，不是语义级导出：不按记录或表选择内容，恢复即整库还原。
  - 与 Import（AI 驱动的语义级写入）和 InstrumentSync（行情同步）是三条互不交叉的数据通道：恢复 ≠ 导入，备份 ≠ 同步。
  - 不含界面状态与偏好（WindowState、ViewState、Appearance、DefaultCurrency 等），那些属设备本地偏好，不随备份迁移。
  - 明文存放，由用户自行妥善保管。
- **别名**：不使用"导出"（语义级、可选择性）、"快照"（偏技术）等词。

## Restore（恢复）

- **定义**：用一份 Backup 快照替换当前数据库的破坏性操作，执行前自动为当前数据创建 RestoreSafetyBackup。
- **边界**：
  - 替换式还原，不是合并式导入（合并属于 Import 通道的职责）。
  - 备份来自更高版本应用时拒绝恢复；来自旧版本时允许，恢复后自动迁移升级。
  - 恢复成功后应用自动重启，以全新状态加载数据。
- **别名**：不使用"导入"（语义级写入，与恢复是两种操作）。

## RestoreSafetyBackup（恢复安全备份）

- **定义**：Restore 执行前，系统自动为当前数据库创建的备份，用于恢复出错时回滚。
- **边界**：由系统自动创建、自动命名，用户无需干预；与用户手动创建的 Backup 存放位置不同。

## BackupDirectory（备份目录）

- **定义**：用户配置的默认备份存放位置；配置后"一键备份"直接写入该目录，无需每次选择。
- **边界**：属设备本地偏好（与 Appearance、DefaultCurrency 同类），不进入 `ledger.db`，也不随 Backup/Restore 迁移。
- **别名**：不使用"导出目录"（备份 ≠ 导出）。

## BackupRetentionLimit（备份保留上限）

- **定义**：用户可配置的受管备份最大保留数量，默认 30，可调范围 1–100。
- **边界**：
  - 属设备本地偏好（与 BackupDirectory 同类），不进入 `ledger.db`，也不随 Backup/Restore 迁移。
  - 只约束 ManagedBackup（受管备份）；ManualBackup（另存备份）不受约束。
  - 上限调小时立即滚动清理到新值；受管备份写入后自动滚动清理。
- **别名**：不使用"最大保存文件个数"（口语化）、"保留策略"（偏宽泛）。

## BackupPruning（备份滚动清理）

- **定义**：把 ManagedBackup 数量修剪到 BackupRetentionLimit 之内的过程：删除最旧的超出部分。
- **边界**：
  - 触发点：任何一次成功写入且落点为受管备份之后；上限调小时立即执行。
  - 排序以备份文件名时间戳为准，解析失败回退文件修改时间。
  - 删除失败的文件跳过并报告，不中断其余清理。
  - 与 RestoreSafetyBackup（恢复安全备份）无关：后者在应用数据目录、命名不同，不受清理影响。

## ManagedBackup（受管备份）

- **定义**：位于配置的 BackupDirectory 内、按自动命名规则（`ledger-backup-YYYYMMDD-HHMMSS.db.zip`）生成的备份文件；受 BackupRetentionLimit 约束。
- **边界**：一键备份与使用默认文件名存入备份目录的"另存为"都会产生受管备份；改名后不属于受管备份。
- **别名**：不使用"自动备份"（易与"自动定时备份"混淆，当前没有定时备份）。

## ManualBackup（另存备份）

- **定义**：用户通过"另存为…"主动选择存放位置或文件名的备份文件。
- **边界**：若写入配置的 BackupDirectory 且文件名匹配自动命名规则，则视为 ManagedBackup；否则不受 BackupRetentionLimit 约束，永不被自动删除。

## TransactionSearch（交易搜索）

- **定义**：用户通过文本关键字检索 `Transaction` 的功能入口，以独立视图呈现（侧边栏"搜索"入口，全局可达），MVP 阶段搜索范围仅限交易。
- **边界**：
  - 可搜索内容：交易备注（`note`）+ 转出账户名 + 对应拼音首字母串（分类名、转入账户名不在索引中）。
  - 匹配语义：整词匹配 + 拼音首字母匹配（如 `cf` 命中「吃饭」）+ 前缀通配（如「吃」命中「吃饭」）；不支持任意位置子串（如「商银」搜不到「招商银行」，可用拼音前缀 `zsyh` 兜底）。
  - 金额、日期不进搜索词；按金额/日期筛选属于筛选器职责，后续另行提供。
  - 搜索结果只读展示（复用交易列表的信息：日期、类型、账户、分类、金额、备注），不做增删改；需要操作时跳回交易列表。
  - 搜索词不持久化（与 ViewState 边界一致）。
  - **时效性**：索引由后台定时刷新维护（默认 60 秒周期，ADR-0004 决策 #14），写入后到下次刷新前新建交易不可搜、软删除交易仍可搜；搜索结果附 `stale` 标志提示索引可能滞后。
- **别名**：不使用"全局搜索"（范围并非全局，仅交易）、"全文搜索"（偏 FTS 技术含义）；"模糊搜索"可作为口语沿用（ADR-0004 文档名），正式术语为"交易搜索"。

## ViewShortcut（视图快捷键）

- **定义**：在应用窗口内用主修饰键（macOS 为 Cmd，Windows/Linux 为 Ctrl）+ 数字键（1–9）直接切换到对应视图的快捷方式，按侧边栏菜单顺序依次对应 9 个视图（概览=1 … 设置=9）。
- **边界**：
  - 仅窗口聚焦时生效，属应用内导航，不是全局快捷键（不劫持系统级按键）。
  - 输入框聚焦时同样生效（Cmd/Ctrl+数字与文本编辑键不冲突）。
  - 弹窗 / 确认框（如分类编辑、删除确认）打开时抑制，避免在编辑或确认中途跳走丢失上下文。
  - 硬编码映射，MVP 不做用户自定义。
- **别名**：不使用“全局快捷键”（那是系统级、应用失焦也生效的概念）。

## 耗时日志（Timing Log）

- **定义**：对数据库执行操作按耗时记录的日志机制，用于建立性能基线、定位慢路径。
- **边界**：
  - 观测单位分两层：SQL 语句级（连接级 hook 全量覆盖，含启动迁移、后台索引刷新、定时引擎）与命令/批次级（span 归因 + 批次汇总）。
  - 默认级别只落超阈值的慢查询；全量明细需 DEBUG 级别（`RUST_LOG=debug`）。
  - 日志遵循隐私约定：默认级别不记录金额等业务值（与 ADR-0006 的 INFO 级约定一致）。
- **别名**：不使用"时间日志"（含糊）、"性能日志"（偏优化手段，本机制只做观测）。

## 慢查询（Slow Query）

- **定义**：单条 SQL 执行耗时超过阈值的语句，阈值默认 100ms。
- **边界**：以 warn 级别记录；启动迁移、后台索引刷新等合法慢路径也会命中，属预期信号而非故障；阈值可随观测数据调整。
- **别名**：不使用"慢 SQL"（口语沿用无妨，正式术语为"慢查询"）。
