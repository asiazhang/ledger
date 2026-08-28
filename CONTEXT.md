# 领域术语表（Ubiquitous Language）

> 本文件记录 Ledger 项目中的核心领域术语。只包含业务概念，不包含实现细节。
>
> 本文件是架构叙述的单一来源；若与代码行为冲突，以代码为准并同步修正本文件。
> 协作约定与文档导航见 `AGENTS.md`；决策记录见 `docs/adr/`。

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
  - 每期金额由分期金额计算规则得出（MVP 决策见 ADR-0024）。
  - 已还金额和已还期数由 `scheduled_transaction_occurrences` 的 `completed` 状态实时汇总。
  - 每次触发时生成一条 `Transaction`（`kind = expense`）。
- **别名**：不使用“loan”、“debt”等词，因为分期不一定是负债（例如分期购买服务）。

## Subscription（订阅）

- **定义**：按周期持续扣款，直到用户手动取消或暂停的 `ScheduledTransaction`。
- **边界**：
  - 每次触发时生成一条 `Transaction`（`kind = expense`）。
  - 生命周期 MVP 决策（无结束日期、无最大期数，仅经 `paused` / `cancelled` 停止）见 ADR-0024。
  - 金额固定：**每一期内**金额固定；价格变更策略 MVP 决策（计划金额不可编辑，价格变化 = 取消旧计划 + 按新金额新建，历史在订阅列表中断为两段真实的价格历史；可编辑字段仅限金额以外的非核心字段，编辑只影响未来期次）见 ADR-0023 决策三。
- **别名**：不使用“membership”、“recurring payment”等词，除非业务明确需要区分。

## SubscriptionSpend（订阅花费）

- **定义**：订阅域的花费口径总称，分**实际花费**与**推算成本**两个互不混用的口径。
- **实际花费**：某时间区间（日历月/日历年）内，由订阅计划期次生成的交易流水的实际支出合计；按流水忠实统计，**不摊销**——年付订阅在扣款月全额计入，其余月份为 0。计划取消/暂停不影响其历史实际花费。
- **推算成本**：按当前 `active` 订阅计划的参数推算的持续烧钱速度，只算 active 计划、不看执行情况；统一折算为**折算月成本**与**折算年成本**（= 折算月成本 × 12）两个数，不做逐月推算明细。
- **折算月成本系数**：月付 ×1、年付 ÷12、周付 ×52÷12、日付 ×30。
- **边界**：
  - 推算成本只作展示，不落库、不进流水与预算。
  - 分期（installment）与定时转账（scheduled_transfer）不属于订阅花费。
  - 不限定软件类订阅，软件/视频/健身等靠分类区分。
- **别名**：不使用"摊销月费"（摊销口径已否决，实际花费不摊销）、"月均花费"（未区分实际/推算）。

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

## InvolvingAccount（涉及账户）

- **定义**：一笔 `Transaction` 与某个账户的关系视角——`account_id`（转出账户）或 `to_account_id`（转入账户）任一命中即算该交易涉及该账户。是交易列表按账户过滤、AI 读回按账户对账的统一口径。
- **边界**：
  - 过滤语义：`account_id = X OR to_account_id = X`；普通收支只命中一端（`account_id`），转账两端（转出 + 转入）都命中。
  - 与"转出账户"对照：转出账户特指 `account_id` 指向的账户（支出/转账的扣款侧、收入的收款侧）；涉及账户更宽，覆盖交易两端——转账的转入侧只在涉及账户语义下可按账户检索到（`to_account_id` 仅转账使用）。
  - 用途：交易页账户过滤（账户名下钻 `?account=<id>` 与手动下拉共用同一语义，issues #97–#99）、AI API `GET /api/v1/transactions` 的 `involving_account_id` 查询参数、AI 读回对账按账户核对（见 AIReadbackVerification）。
  - 是既有 `account_id`（仅转出账户）过滤之外**新增**的维度，不改旧字段语义（发布冻结只增不改）。
- **别名**：不使用"相关账户"（含糊）、"关联账户"（易与数据库外键"关联"混淆）。

## Plan Lifecycle（计划生命周期）

- **定义**：`ScheduledTransaction` 的计划状态集合（`active` / `paused` / `cancelled` / `completed`）与状态变更规则，决定新期次的生成与既有期次/交易的去留。
- **MVP 决策**：状态集合、取消/暂停副作用（取消不删已生成交易、暂停不改已生成期次）与不支持项（单独取消/跳过某期、修改单期金额或日期）见 ADR-0024。

## Timing（时间精度）

- **日期精度**：所有定时交易只精确到日期，不记录具体执行时间。
- **执行日期**：`Occurrence` 的 `scheduled_date` 为 ISO 8601 日期格式（YYYY-MM-DD）。
- **节假日处理**：MVP 采用严格日期（不顺延）的决策见 ADR-0024。
- **边界**：`Transaction.date` 直接复用 `Occurrence` 的 `scheduled_date`，两者保持一致。

## Counterparty（交易对手）

- **定义**：定时交易中的收款方或付款对象，例如商家、贷款机构、订阅服务商。
- **MVP 决策**：`counterparty` 字段落点（计划扩展表）、生成交易的复制方式、不在 `Transaction` 表新增通用字段及 `ScheduledTransfer` 不使用 `counterparty`（用 `to_account_id` 表示本方账户间转账）的决策与边界见 ADR-0024。

## Amount Model（金额模型）

**raw/native 分离（`transactions` 行级）**：
- `amount_cents`：原始币种金额；`amount_native_cents`：本位币金额（折算到全局默认币种 DefaultCurrency）。
- 折算由 `transaction::amount::convert_to_native` 统一执行：与默认币种相同 → 1:1；否则按汇率折算（正反向汇率兜底），缺汇率报错、不静默混币种。MVP 阶段多币种汇率 1:1，故二者恒等。
- 折算基准为全局默认币种、与账户币种无关，避免跨账户汇总口径漂移。
- 四个具名度量（`account_flow` / `expense_net` / `income_net` / `refund_gross`）对 8 种 kind 的符号归属见 Transaction Kind Mapping。

**分期金额计算（ScheduledTransaction）**：每期金额固定的 MVP 决策与尾差计算规则见 ADR-0024「分期金额计算规则」（MVP 不支持每期金额不同、不支持 subscription 中途涨价）。

## Recurrence Rule（周期规则）

- **定义**：定时交易用显式字段（`recurrence_type` 周期类型 / `recurrence_interval` 间隔 / `recurrence_day` 具体日期或星期）表达的周期，如每 1 个月、每周一。
- **MVP 决策**：显式字段而非 RRULE、仅支持常见固定周期（复杂规则留到后续版本）见 ADR-0024。

## Failure Policy（失败策略）

- **定义**：期次执行失败即标记为 `failed`、由用户手动重试的处理约定。
- **MVP 决策**：不自动重试、不自动跳过、不产生滞纳金及其理由见 ADR-0024。

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
  - **暴露的接口**（11 个端点）：`openapi.json`、`accounts`（list/create/delete）、`accounts/balances`（含黑洞账户）、`categories`（list/create/delete）、`transactions`（list，可按日期/转出账户/涉及账户/类型过滤）、`transactions/batch`、`transactions/{id}`（delete/update）、`currencies`（list）、`import/knowledge`。
  - `accounts` / `categories` 的 create 按自然键幂等（同名复用已有记录）；`transactions/batch` 支持 `dedup` 参数（默认开启）与客户端 `idempotency_key`（见 ImportDedup / IdempotencyKey）。
  - `import/knowledge` 返回精简的导入约定文本（Pixiu 列映射、转账拆分、黑洞账户、币种映射、分单位、日期、dedup），供 AI 直接注入系统提示词。
- **别名**：不使用"本地 API"（过于泛化）、"后端 API"（与 Tauri IPC 混淆）。

## AIReadbackVerification（AI 读回验证）

- **定义**：AI 编程助手完成批量导入后，通过读回接口核对迁移结果是否完整的环节——用 `GET /api/v1/transactions` 按日期区间/转出账户/涉及账户/类型过滤读回交易，核对源文件各行是否全部落库、金额合计是否一致；再用 `GET /api/v1/accounts/balances` 拿到各账户（**含黑洞账户**）实时余额，核对期末余额与源数据吻合。
- **边界**：
  - 读回是查询能力：`transactions` 返回未删除交易（按 `date DESC` 排序），`balances` 口径 = 初始余额 + Σ `account_flow`（各 kind 符号归属见 Transaction Kind Mapping，含投资类），实时计算不持久化。
  - 对账要点：余额清单包含黑洞账户，可识别误挂到 `无` 的交易；转账按转出账户对账，需核对转入侧时改用涉及账户过滤读回（`involving_account_id`，`account_id` 或 `to_account_id` 命中即算，见 InvolvingAccount）。
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
  - **投资状态（`invested`）**：列表返回的派生布尔字段，= 是否为持仓标的（见 InvestedInstrument）；标的页据此展示"持仓"标记列，并可用"只看持仓"开关过滤（`only_invested` 过滤参数，issue #102）。
- **别名**：不使用"证券"（偏法律概念）、"产品"（过于泛化）。不单独使用 "market_symbol" 或 "ticker"——统一使用 "symbol"。

## MarketPrice（市场价格）

- **定义**：某个 Instrument **当前**的市场报价——现价缓存，即 PriceHistory 中该标的最新一条的即时映像，供持仓市值与未实现盈亏（Holding）计算消费。
- **边界**：
  - 仅代表"现在"：每次同步覆盖更新本条记录；历史报价序列由 PriceHistory 承载，MarketPrice 不再是价格的唯一载体。
  - `price_cents` 和 `currency_code` 由数据源（东方财富）返回，币种按市场固定：`sh`/`sz` → `CNY`，`hk` → `HKD`。
  - `priced_at` 记录同步时间。
  - `source` 标记数据来源，当前为 `eastmoney`。
- **别名**：不使用"行情"（偏实时交易概念）、"报价"（偏询价场景）、"最新价快照"。

## PriceHistory（价格历史）

- **定义**：某个 Instrument 的市场报价按周粒度累积的历史序列，用于绘制单标的走势与投资资产走势（PortfolioValueTrend），是价格在「现价」之外的第二条承载线。
- **边界**：
  - **仅覆盖股票类持仓标的**（口径同 InvestedInstrument）：基金、债券等无行情来源的标的不参与价格记录与走势图（MVP 决策，非股票仅股票/ETF 走东财日 K 数据源）。
  - **周采样**：每周至多记录一条，取该周最后一个有报价交易日的价格；整周无有效报价（停牌、节假日全周覆盖等）则该周无点，曲线跨越空档连续。一周内重复获取时以新数据**整周覆盖**（幂等），不产生重复。
  - **采集单通道**：经 K 线接口一次性回填近两年，此后随持仓价格增量同步持续累积；日线最后一根即今日最新价，故"回填历史"同时也是"刷现价"，不存在两条刷新通道。
  - 已清仓标的的价格历史保留不删，供回看过去的组合市值；清仓只影响后续不再新增。
  - 港股以 HKD 计价，跨币种汇总走势须经 FxRateHistory 同期折算，不用当前汇率近似历史。
- **别名**：不使用"历史行情"（口语偏全市场）、"K 线"（那是数据源的原始形态，本地不保留日线）、"净值曲线"（基金净值语义不同）。

## FxRateHistory（汇率历史）

- **定义**：货币间兑换比率按周累积的历史序列，与 PriceHistory 同源同时段采集（东方财富汇率日 K），用于把非默认币种的**历史**市值折算到 DefaultCurrency。
- **边界**：
  - 与现有 ExchangeRate（当期汇率表）并存分工：当期折算走 ExchangeRate，历史期折算走 FxRateHistory；同期同规则（正反向兜底）。
  - 周采样与整周覆盖规则与 PriceHistory 一致。
  - 只随投资域市值折算消费；MVP 阶段交易流水折算（raw/native）仍只用当期汇率，不变。
- **别名**：不使用"历史汇率表"（易与当期表混淆指代）；可简称"汇率历史"。

## PortfolioValueTrend（投资资产走势）

- **定义**：把用户全部（股票类）持仓在各时点的市值汇总成的时间序列视图，回答"我的投资总价值如何变化"；同一功能内也呈现单个标的自身价格序列的单标的走势。
- **边界**：
  - 组合市值 = 各持仓标的「当期持有数量 × 当期价格」之和：数量由 buy/sell 交易流水在查询时推算（不物化每日/每周快照），价格取自 PriceHistory。
  - 跨币种按 FxRateHistory 折算为 DefaultCurrency 后汇总成一条曲线；曲线为周采样点连线。
  - 展示位置 MVP 为投资页内的走势视图（预设区间切换：1 月 / 3 月 / 1 年 / 全部；组合走势与单标的走势同视图切换）；多标的叠加对比属后续迭代。
  - **不含现金账户**：总资产走势（现金余额历史重建 + 投资市值）是明确的后续迭代，不在本概念范围内。
  - 标的在区间起点之前买入无早期价格时，曲线从首个有效采样点开始。
- **别名**：不使用"净值走势"（基金净值语义不同）、"收益曲线"（混入收益率口径）、"总资产走势"（明确含现金账户的后续概念）。

## Holding（持仓）

- **定义**：用户在某个投资账户中持有的某个 Instrument 的数量及成本信息。
- **边界**：
  - `quantity` 为当前持有数量（由各持仓批次 `remaining_quantity` 实时聚合）。
  - `cost_basis_cents` 为持有成本（买入总金额），`cost_currency_code` 为成本币种。
  - `latest_price_cents`、`market_value_cents`、`unrealized_pnl_cents` 由 `v_holdings` 视图实时计算（关联最新 MarketPrice 与汇率折算到账户本位币），不落库存储。
  - 市值 = quantity × latest_price_cents（按汇率折算）；未实现盈亏 = 账户币市值 - 账户币成本。
- **别名**：不使用"仓位"（偏交易术语）、"库存"（偏实物）。

## NetWorth（净资产）

- **定义**：用户全部真实财富在某一时刻的单一合计数字（本位币）：Σ 非投资账户余额 + Σ 持仓市值，均折算为 DefaultCurrency 后相加。
- **边界**：
  - 由后端只读聚合命令 `dashboard_overview` 实时计算，不落库存储；账户侧沿用 `account_flow` 余额口径（与账户列表/余额页一致，排除隐藏/黑洞账户），投资账户余额不计入——其价值经持仓市值体现，避免同一笔资产重复计算。
  - 持仓市值取 `v_holdings` 的 `market_value_cents`（账户本位币），再折算到 DefaultCurrency；从未录价（或缺折算汇率）的持仓按空值语义跳过，不以零计入。
  - 折算遇缺失汇率让错误上抛（中文错误信息），不静默返回不完整的合计数字。
  - 负债账户按 `account_flow` 现行符号忠实求和，不在净资产层强制取负。
  - 「总资产走势」（净资产的时间序列，现金余额历史重建 + 市值）是明确的后续迭代，不在本概念范围内。
- **别名**：不使用"总资产"（与「总资产走势」既有预留概念区分，避免同一页面两套口径混淆）、"净值"（基金净值语义不同）。

## Investment（投资域）

- **定义**：承载证券交易（buy/sell）背后持仓批次、卖出匹配与已实现盈亏的概念域，物理实现为 `commands::investment` 模块（lot / 匹配 / pnl 数据逻辑）。
- **边界**：
  - **buy/sell 首先是交易 kind**：一笔 buy/sell 先是一笔 `Transaction`（交易行落库经 Writer 接缝），Investment 是它背后的持仓/盈亏载体。
  - 对外出口收窄为 `prepare / apply / revert` 三件套（issue #72）：prepare 校验归一化（不落库）、apply 应用副作用（buy 建仓 / sell 卖出匹配）、revert 回退副作用（buy 守卫+清理 / sell 回补）；交易行写入由交易行为层编排（经 `transaction::writer`），Investment 不再反向依赖 transactions 的行更新函数（双向依赖已斩断，issue #70）。
  - 分派用薄而穷尽的 `match`，不引入 trait 注册表（避免过度设计）。
- **别名**：不使用“投资账户”（那是 `AccountType::Investment` 账户）、“证券模块”（偏数据层）。

## InvestedInstrument（持仓标的）

- **定义**：有**当前持仓**（`security_lots.remaining_quantity > 0`，即 `v_holdings` 视图有行、且排除软删除账户的批次）的 Instrument，即“已投资”标的（ADR-0015）。
- **边界**：
  - 判定谓词单点定义（`commands::investment::crud` 的 `INVESTED_EXISTS`），同一口径驱动四处：`list_instruments` 的 `invested` 派生列、标的页"只看持仓"过滤（`only_invested`）、增量同步（`sync_holding_prices`）的标的集合、盈亏页持仓概览（issue #102/#103/#110）。
  - **不含已清仓标的**：已清仓的得失由已实现盈亏（RealizedPnl）承载，不混入“已投资”。
- **别名**：不使用"已投过的标的"（含已清仓）、"自选/关注标的"（那是用户主观收藏，与持仓无关）。

## InstrumentSync（标的全量同步）

- **定义**：用户手动触发、系统从东方财富 API 一次性全量拉取沪市/深市/港股股票标的信息（代码、名称、最新价），upsert 到本地 `instruments` 与 `market_prices` 的过程。**职责 = 修标的字典**（ADR-0015 职责切分：全量修字典 / 增量刷价格 + 沉淀历史；ADR-0019 价格历史化）。
- **边界**：
  - 触发方式：手动触发（标的页"全量同步"入口，issue #101 迁移后设置页不再提供同步入口），不做自动定时刷新。
  - 同步范围：沪市（`sh`）、深市（`sz`）、港股（`hk`），币种分别固定为 `CNY`/`CNY`/`HKD`。
  - 执行方式：一次性全量同步、分页拉取，无并发限制、无失败重试。
  - 去重：按 `symbol` 匹配已有标的，名称或市场变更则更新，不存在则插入，并同步 upsert 该标的最新价到 `market_prices`。
  - 进度反馈：通过 `sync-instruments:progress` 事件向前端汇报当前页数/总数与累计新增、更新数量。
  - **中断机制（issue #104）**：分页循环每页检查共享取消标志（`AtomicBool`），经 `cancel_sync_instruments` 命令置位后即提前返回并推送中断态事件；已落库数据保留（upsert 幂等）、下次重跑自动续上。进度事件终态以 `cancelled` 字段区分完成（`done=true, cancelled=false`）与中断（`done=true, cancelled=true`）。
  - 无同步进行时调用 `cancel_sync_instruments` 无副作用，返回明确提示。
- **与增量同步的边界**：全量修字典（增删改 `instruments` + 刷价），增量刷价格并沉淀历史（见 HoldingPriceSync、ADR-0019）；日常刷新现价与走势走增量同步，全量仅在标的字典需要修补时触发（二次确认说明范围与数百次 API 请求代价）。
- **别名**：不使用"同步价格"（偏持续同步）、"更新行情"（偏实时）。

## HoldingPriceSync（持仓价格增量同步）

- **定义**：用户手动触发、从当前持仓（口径同 InvestedInstrument）收集股票类标的，按 `(market, symbol)` 构造东财 secid（`1.600519` / `0.000001` / `116.00700`）批量查询最新价（`ulist.np/get`，约 50 只/请求），并按日 K 接口回填近两年周线历史：upsert `market_prices`（现价）+ 把日线降采样落 PriceHistory 与 FxRateHistory（ADR-0019）——不增删、不改 `instruments` 的名称与市场。UI 文案为"同步持仓价格"，与"全量同步"区分（ADR-0015 职责切分：全量修字典 / 增量刷价格 + 沉淀历史）。
- **边界**：
  - 触发方式：手动触发（标的页"同步持仓价格"按钮与盈亏页"当前持仓"概览卡右上角按钮，两处共用同一 `useHoldingPriceSync` 接缝、行为一致），不做自动定时刷新。
  - 只覆盖股票类持仓：非股票持仓（基金/债券等，数据源不含行情）、市场未知（无法构造 secid）的标的计入"跳过 M 只"统计提示，不报错。
  - 停牌/无效价（f2≤0）跳过、保留旧价，不中断同步；响应 data:null（全部代码无效）优雅降级为空。
  - 重复触发幂等：每标的一条 `market_prices` 覆盖更新（`source` 沿用 `eastmoney`）；PriceHistory 同周整周覆盖，均不产生重复数据。
  - 完全无持仓时返回明确提示（"无持仓标的可同步"）而非报错。
  - 复用全量同步的 HTTP 层（主机池、重试、限流 pacer）与价格换算（A 股 f2 直接得分、港股 ÷10），增量同步 API 访问量从全量数百次降到个位数（issue #103）。
- **别名**：不使用"增量行情"（口语），正式术语为"持仓价格增量同步"；UI 文案固定为"同步持仓价格"。

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

- **定义**：账本数据库的完整文件级快照，产物为包含数据库文件与元数据（备份时间、应用版本、schema 版本、来源标记 `kind`）的 zip 包。触发方式分两种：用户手动触发（ManualTrigger 语境，见 BackupTrigger / ManagedBackup），或系统自动定时触发（AutoBackup）。
- **边界**：
  - 是文件级快照，不是语义级导出：不按记录或表选择内容，恢复即整库还原。
  - 与 Import（AI 驱动的语义级写入）和行情同步（InstrumentSync 全量同步 / HoldingPriceSync 增量同步）是三条互不交叉的数据通道：恢复 ≠ 导入，备份 ≠ 同步。
  - 不含界面状态与偏好（WindowState、ViewState、Appearance、DefaultCurrency 等），那些属设备本地偏好，不随备份迁移。
  - 明文存放，由用户自行妥善保管。
  - 产物内容与格式由手动与自动共用一套机制（VACUUM INTO 快照 + zip 打包，见 ADR-0007 / ADR-0016）；两类备份仅触发来源与命名前缀不同，并以元数据 `kind: "auto"|"manual"` 显式区分来源（issue #127）；旧版本备份缺该字段时按 "manual" 处理，列表与恢复不报错。
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
  - 手动与自动触发的 ManagedBackup 同等对待、共享同一上限：最旧淘汰，不区分来源（ADR-0016）。
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

- **定义**：位于配置的 BackupDirectory 内、按受管命名规则（`ledger-backup-YYYYMMDD-HHMMSS.db.zip` 手动 / `ledger-auto-YYYYMMDD-HHMMSS.db.zip` 自动，见 BackupTrigger）生成的备份文件；受 BackupRetentionLimit 约束。
- **边界**：
  - 来源分两类：手动（一键备份 / 使用默认文件名存入备份目录的"另存为"）与自动（AutoBackup）。
  - 两类受管备份在配额与滚动清理上同等对待（共享 BackupRetentionLimit，最旧淘汰，不区分来源，ADR-0016）。
  - 改名后不属于受管备份；不受管（另存到其它位置或改名）的文件永不被自动清理。
  - 受管判定按文件名前缀（`ledger-backup-` / `ledger-auto-`）识别：后端 `MANAGED_BACKUP_PREFIXES` 与前端 `isManagedBackupPath` 各持一份常量并保持一致（ADR-0016 已接受该取舍）。
- **别名**：不使用"自动备份"（那是 BackupTrigger 的自动来源，不是"受管"的同义）。

## ManualBackup（另存备份）

- **定义**：用户通过"另存为…"主动选择存放位置或文件名的备份文件。
- **边界**：若写入配置的 BackupDirectory 且文件名匹配自动命名规则，则视为 ManagedBackup；否则不受 BackupRetentionLimit 约束，永不被自动删除。

## BackupTrigger（备份触发来源）

- **定义**：区分一次 Backup 由谁发起的概念维度——**手动**（用户经设置页"一键备份/另存为"主动触发）或**自动**（AutoBackup 引擎按调度规则触发）。
- **边界**：
  - 手动/自动只影响命名前缀（`ledger-backup-` / `ledger-auto-`）与 zip 内 `backup.json` 的 `kind` 字段（`manual` / `auto`）；产物格式、恢复流程、受管属性完全一致（ADR-0016）。
  - 恢复安全备份（RestoreSafetyBackup）是系统自动创建但**不属于** AutoBackup（不经调度器、不标脏、不占配额、独立存放位置）。
- **别名**：不使用"来源"（过于宽泛）、"自动/手动备份"口语（正式术语为"备份触发来源"）。

## AutoBackup（自动备份）

- **定义**：由应用内置调度器按周期自动触发的 Backup，目标是让"数据有变化时不至于长期无备份"——用户忘了手动备份也不丢数据。触发条件为"距上次备份超过间隔（24 小时）且自上次备份以来数据有变化（DirtyMarker）"。
- **边界**：
  - 间隔固定为 24 小时（ADR-0016），锚点是"距上次备份"而非固定时刻；检查周期短（30 分钟轮询 + 写时顺带检查）但备份频率上限是每天一次。
  - 只在应用运行期间生效；系统休眠时调度暂停，唤醒后由短周期轮询在 30 分钟内补上。应用退出时若脏则兜底备份一次（不受每日约束）。
  - 首次启动若备份列表为空（不分手动/自动）则立即备份一次，保证装上当天就有一份。
  - BackupDirectory 未配置时自动备份静默不执行，设置页提示引导配置。
  - 自动备份的开关与调度状态（DirtyMarker、下次到期时间、上次备份时间）存于 `ledger.db` 的 AppSettings 表（后端权威，ADR-0016/0017），随 Backup/Restore 迁移；`backupDir`/`backupMaxCount` 仍属设备本地偏好。
  - 失败不重试（保留 DirtyMarker，下个周期重试），成功不通知用户；产物同为 ManagedBackup，参与滚动清理。
  - 备份产物变更（自动备份完成 / 受管备份清理）成功后发出无 payload 的 `ledger:backups-changed` 信号（issue #129，与参考失效信号 `ledger:changed` 平行）；前端设置页订阅该信号自动刷新备份列表与自动备份状态。
- **别名**：不使用"定时备份"（偏计划任务语义）、"自动定时备份"（啰嗦，正式术语"自动备份"）。

## DirtyMarker（脏标记）

- **定义**：记录"自上次备份以来数据是否有变化"的布尔状态，是 AutoBackup 决定"到点是否真正执行备份"的唯一依据。
- **边界**：
  - 置真：任何一次业务写库成功后（交易写入经 Writer 接缝、参考数据 CRUD、市场数据写入），显式调用；备份成功后清真。
  - 恢复（Restore）成功后重置：恢复本身生成了 RestoreSafetyBackup，数据刚被校验，不产生"恢复后立即备份"。
  - 调度器自身的状态写入（开关/到期时间）不置真，避免自触发。
  - 失败保留：备份失败不清真，下个周期重试（ADR-0016）。
  - 属调度状态，存于 AppSettings 表（`auto_backup.dirty` 键）。
- **别名**：不使用"脏位"（偏底层实现）、"变更标记"（含糊）。

## AppSettings（应用配置 KV 表）

- **定义**：后端权威的应用配置与运行时状态的唯一持久化落点：`ledger.db` 内一张通用 KV 表 `app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)`（ADR-0017）。key 以 `<feature>.<name>` 点分命名、在 Rust 侧由 `settings.rs` 的枚举集中定义；值用 serde_json 序列化、类型由读取方声明，key 缺失或表缺失时返回默认值。
- **边界**：
  - 谁消费谁权威：前端独享消费的设备偏好（Appearance、BackupDirectory 等）→ localStorage；后端消费或随 Backup/Restore 迁移的配置与运行时状态 → 本表；需关系结构的实体（多行、可查询、外键）→ 才配独立表。单行状态专表不再出现。
  - 读写收口在 `src-tauri/src/settings.rs` 的 `get<T>` / `set<T>` 接缝，禁止散落字符串字面量 SQL 与 key。
  - 不透传给前端：对外 IPC 保持领域命令形状（聚合多个 key 返回类型化 DTO），不做通用 get/set_setting；写路径是行为不是赋值。
  - 表随迁移创建，旧版本备份恢复后缺表即取默认值，行为免费正确。
- **别名**：不使用"配置文件"（独立 JSON/TOML 已否决，DataLocation 引导文件是唯一例外，见 ADR-0018）、"单行状态表"（否决方案）。

## 轻量设置项（Lightweight Setting）

- **定义**：只被前端消费、持久化在 localStorage 的设备偏好。判定标准（三条同时满足）：① 读写不需要后端命令参与；② 不随 Backup/Restore 迁移；③ 改动即时生效、只影响本设备（主题、默认币种、备份目录、备份保留上限等）。与 AppSettings（后端消费或随备份迁移，存 SQLite KV 表）互斥，两套落点不交叉（ADR-0017）。
- **边界**：
  - **轻量只决定可否合并，归属领域决定合到哪**（ADR-0022）：轻量是进设置页「通用」Tab 的资格线——纯设备偏好（如深色模式、默认币种）才可并入通用；若其领域归属是参考数据或数据文件管理，则留在对应领域 Tab 内（如备份目录虽轻量，但归属数据文件管理，留在「数据」Tab 的备份卡片内）。
  - 设置页 Tab 分域（ADR-0022）：通用（轻量设备偏好）→ 分类与币种（参考数据）→ 数据（数据文件管理）→ 关于（恒在末位）。
- **别名**：不使用"本地配置"（与 DataLocation 引导文件混淆）、"用户偏好"（过泛，未区分消费方）。

## DataLocation（数据存储位置）

- **定义**：ledger 主数据库文件所在目录。未配置时使用系统应用数据目录；用户可更改位置，由应用把现有数据库完整搬迁到新位置（Relocation）。同一时刻全库只此一份。
- **边界**：
  - 单一活动路径：不支持多套账本并存切换；只指定目录，文件名固定为 `ledger.db`。
  - 位置的记录属**引导配置**：必须在打开 `ledger.db` 前被读取，因此不可能存入库内（进不了 AppSettings 表），也无法经前端推送（Rust 启动早于前端就绪）；它是「独立配置文件已否决」规则的**唯一例外**（ADR-0017/0018），只能位于设备本机的固定位置。
  - 属设备本地偏好（与 BackupDirectory 同类），不随 Backup/Restore 迁移；Restore 永不恢复此项——这天然保证换机不会指向不存在的路径，失效时回退默认位置并显著提示，原库永不被静默改动或删除。
  - 更改位置**重启后生效**：启动引导发现目标位置无库而原位置有库时，自动执行 Relocation；目标位置已有同名库须用户显式确认接管，不静默覆盖。
- **术语区分**：Relocation（搬迁，整个库文件移至新 DataLocation）≠ schema 迁移（备份恢复后的表结构版本升级），两词不可混用。
- **别名**：不使用"数据库路径"（口语化，且易误解为完整文件路径）、"多账本/工作区切换"（明确不是本功能）。

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
  - 弹窗 / 确认框（如分类编辑、删除确认）打开时抑制（判定机制见 Overlay Suppression），避免在编辑或确认中途跳走丢失上下文。
  - 硬编码映射，MVP 不做用户自定义。
- **别名**：不使用“全局快捷键”（那是系统级、应用失焦也生效的概念）。

## CreateShortcut（记一笔快捷键）

- **定义**：交易页裸键 `a`/`z`/`i`/`b`/`s` 直达对应类型的记一笔弹窗（支出/转账/收入/买入/卖出），随视图装卸仅交易页生效。
- **边界**：
  - **仅裸键，不用 Cmd/Ctrl 组合**（2025-08 决策）：组合键让给系统与视图快捷键（如 Cmd+Z 撤销、Cmd+A 全选的肌肉记忆）；裸键失效属于弹层误判 bug，修弹层判定而非加组合键双轨。
  - 退款（refund）不占键位，入口由交易条目右键菜单承接。
  - 焦点在可编辑元素或弹层打开时抑制（见 Overlay Suppression）。
- **别名**：不使用“组合键快捷键”、“记一笔快捷键入口”（口语沿用无妨，正式术语为记一笔快捷键）。

## Overlay Suppression（弹层抑制）

- **定义**：弹层（弹窗、确认框、气泡确认、下拉菜单、筛选下拉）打开期间抑制全局快捷键的机制，视图快捷键与记一笔快捷键共用同一判定。
- **边界**：
  - 判定信号是**仅打开时存在**的 DOM 元素（模态遮罩、日历面板、下拉/选择菜单体），不是弹层容器——Naive UI 的弹层容器关闭后空壳永久残留 DOM（懒传送门语义），以容器存在性为信号会把「已关闭」误判为「打开」，导致快捷键永久失效（见 ADR-0021）。
  - 弹层关闭动画期间（遮罩淡出未完）仍视为打开，属预期。
- **别名**：不使用“弹窗检测”、“覆盖层探测”。

## ESC 键语义

- **定义**：ESC 是应用内部职责的按键——有弹层时由 naive-ui 默认行为关闭最上层弹层，无弹层时无操作；**永不作用于窗口层**（不退出全屏）。窗口行为守卫（issue #154）在 document 捕获阶段无条件 `preventDefault`。
- **边界**：
  - 拦截无条件，不区分全屏状态：JS 层检测不到 macOS 原生全屏（WKWebView 中 fullscreen DOM API 恒空），且按语义 ESC 本来就不归窗口层管。
  - 只 `preventDefault` 不阻断传播：naive-ui 弹层（NModal closeOnEsc）依赖事件继续传播。
  - 退出全屏只走系统手势（顶部指针 / Ctrl+Cmd+F）；全屏状态重启后由 window-state 插件恢复（见 WindowState）。
- **别名**：不使用“ESC 退出全屏”（那是被禁止的默认行为）。

## 原生右键菜单

- **定义**：WKWebView/WebView 自带的默认上下文菜单（Back/Reload 等），与记账应用无关。窗口行为守卫（issue #154）默认 `preventDefault` 禁用之。
- **边界**：
  - 唯一例外：可编辑元素内放行，保留系统编辑菜单（剪切/拷贝/粘贴）。判定复用记一笔快捷键的 isEditableTarget（input/textarea/select/contenteditable）。
  - 交易页行级自定义右键菜单（issue #151）不受影响：拦截只 `preventDefault` 不阻断传播，行级 NDropdown 仍能读取事件坐标弹出。
  - macOS 无原生禁用 API（Windows 有），前端 `preventDefault` 是唯一跨平台手段。
- **别名**：不使用“浏览器右键菜单”、“WebView 菜单”（正式术语为原生右键菜单）。

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

## Item（物品）

- **定义**：用户拥有并使用的耐用实物（如手机、电脑、家具、车、家电），用于跟踪它"从购买到今天平均每天花多少钱"（使用成本摊薄）。是账本中与参考数据（账户/分类/币种）、交易流水（Transaction）、投资标的（Instrument）并列的独立领域概念，不是字典也不是一次性交易。
- **边界**：
  - 仅覆盖耐用实物，不含服务/订阅（那些是 ScheduledTransaction / Subscription 的语义，见 Subscription）。
  - 每个物品**自包含总成本**（`total_cost_cents`）：基础成本在有关联购买交易时自动带出，后续花费（维修/配件/运费）由用户编辑总数累加；**不**由系统对多笔交易求和（避免与账本产生双向纠缠，见 Item↔Transaction 关联）。
  - 可选关联一笔购买交易，仅为自动带出成本/日期与溯源；不建立"交易→物品"的反向引用，也不要求有对应交易（赠品/旧物允许只记录物品、不产生交易）。
  - 生命周期：`in_use`（在用）/ `disposed`（已处置，记录处置日期 + 可选残值）。
  - 币种/金额复用 Amount 接缝（整数分 + raw/native 折算到默认币种）。
  - 属独立领域（独立 store），不属于参考数据（那不是"可选值字典"）；写入后发 `ledger:changed`（粗粒度失效）。
- **别名**：不使用 "Asset / 资产"（偏会计/金融，且与投资域混淆）、"Good"、"Instrument"（那是金融标的，见 Instrument）。

## DailyUsageCost（每天使用成本）

- **定义**：一件 Item 的平均使用成本，= 该物品分摊的总成本 ÷ 拥有天数（从购买日期到今天，或用户自选参考日；已处置物品到处置日，可选扣残值）。展示为本位币（默认币种）。
- **边界**：
  - 分子 = 物品总成本（基础 + 追加）；已处置且填残值时分子 = 总成本 − 残值；分子下限 0（残值 ≥ 成本或总成本异常为负时不输出负成本）。
  - 分母 = 自购买日期到"目前"的日历天数，默认取今天实时，支持用户自选参考日。
  - 只输出"每天"一个口径（月/年由用户心理换算，避免口径发散）。
- **别名**：不使用"摊销成本"（偏会计）、"折旧"（会计上特指固定资产折旧，此处是主观使用成本）。

## 万分位分组（展示层口径）

- **定义**：所有数字展示的统一读数口径——整数部分从右向左每 4 位一组、半角逗号分隔（`1234567.89` → `123,4567.89`）；小数部分连续输出不分组、去掉全部小数尾零（`98.00` → `98`、`98.50` → `98.5`，小数位全空时连小数点一起去掉），负号恒在最前。全 App 固定采用，不做设置项。
- **边界**：
  - 仅作用于**展示层**字符串（金额 `formatAmount` 与数量 `formatQuantity` 共享同一核心助手）：存储仍为 `_cents` 整数分、输入解析与导出等机器可读路径不受影响、Rust 后端零改动。
  - ≤4 位整数天然不受影响；不同币种按各自小数位格式化后再对整数部分分组。
  - 数量列（股数/份额）走同一分组规则；带小数的份额仅整数部分分组。
- **别名**：不使用"千分位"（那是西文每 3 位一组的习惯，与本口径冲突）。
