# 领域词汇表：核心交易

> Ledger 领域词汇表的核心交易分域。全部分域与彼此关系见 `CONTEXT-MAP.md`；决策记录见 `docs/adr/`。
> 本分域定义被所有域消费的共享术语（Transaction、Amount Model、Transaction Kind Mapping、Category、DefaultCurrency 等）；其他分域以「见核心交易域 X」引用，不复制定义。
> 若与代码行为冲突，以代码为准并同步修正本文件。

## Transaction（交易流水）

- **定义**：一笔实际发生的资金变动，已存在于 V001；在定时交易语境下，它是定时计划域 `ScheduledTransaction` 的某期执行产物。
- **kind（交易类型，8 种，真源为 `transaction::amount::TransactionKind` 枚举）**：
  - `income`（收入）/ `expense`（支出）：日常收支。
  - `transfer`（转账）：`account_id` 转出、`to_account_id` 转入。
  - `refund`（退款）：关联原支出交易（`refund_of_transaction_id`），账户/币种/分类继承原支出。
  - `buy`（买入证券）/ `sell`（卖出证券）：资本变动，关联投资持仓（见投资域 Instrument / Holding）。
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

## TransactionInput 装配器

- **定义**：前端「记一笔」写入路径的唯一装配接缝——各交易表单形态（支出/收入、转账、退款、买入/卖出）的表单状态进、完整 `TransactionInput` 出（issue #167）。同步纯函数；商户解析与语义校验不在此接缝（留表单层）。
- **边界**：
  - **per-kind 字段矩阵**：各 kind 填什么、留什么 null（含 buy/sell 占位语义：`amount_cents` 置 0、关联字段全 null；refund 的 null 占位）收敛于矩阵一处；新增表单形态只改矩阵。
  - **金额转换单点**：元转分全仓只有 `yuanToCents` 一处实现（`toFixed(8)` 消浮点误差口径），任何表单不再手写 `Math.round(×100)`。
  - **日期转换单点**：`toLocalDateISO` 本地日历日语义（用户所选日的本地零点取日期），不走 UTC 截断——本地 0–8 点记的账归属当天。
  - **两路同构**：同一装配结果供创建与修改共用；非法输入 fail fast 显式报错、不静默兜底（与 dividend/split 的「显式拒绝」同一原则）。
  - **与 Writer 接缝的关系**：装配器是前端 in-process 唯一新增接缝，落库侧继续走 Writer 接缝（wire 契约不动）。
- **别名**：不使用「表单装配层」「输入构造器」。

## InvolvingAccount（涉及账户）

- **定义**：一笔 `Transaction` 与某个账户的关系视角——`account_id`（转出账户）或 `to_account_id`（转入账户）任一命中即算该交易涉及该账户。是交易列表按账户过滤、AI 读回按账户对账的统一口径。
- **边界**：
  - 过滤语义：`account_id = X OR to_account_id = X`；普通收支只命中一端（`account_id`），转账两端（转出 + 转入）都命中。
  - 与"转出账户"对照：转出账户特指 `account_id` 指向的账户（支出/转账的扣款侧、收入的收款侧）；涉及账户更宽，覆盖交易两端——转账的转入侧只在涉及账户语义下可按账户检索到（`to_account_id` 仅转账使用）。
  - 用途：交易页账户过滤（账户名下钻 `?account=<id>` 与手动下拉共用同一语义，issues #97–#99）、AI 导入域 `GET /api/v1/transactions` 的 `involving_account_id` 查询参数、AI 读回对账按账户核对（见 AI 导入域 AIReadbackVerification）。
  - 是既有 `account_id`（仅转出账户）过滤之外**新增**的维度，不改旧字段语义（发布冻结只增不改）。
- **别名**：不使用"相关账户"（含糊）、"关联账户"（易与数据库外键"关联"混淆）。

## Amount Model（金额模型）

**raw/native 分离（`transactions` 行级）**：
- `amount_cents`：原始币种金额；`amount_native_cents`：本位币金额（折算到全局默认币种 DefaultCurrency）。
- 折算由 `transaction::amount::convert_to_native` 统一执行：与默认币种相同 → 1:1；否则按汇率折算（正反向汇率兜底），缺汇率报错、不静默混币种。MVP 阶段多币种汇率 1:1，故二者恒等。
- 折算基准为全局默认币种、与账户币种无关，避免跨账户汇总口径漂移。
- 四个具名度量（`account_flow` / `expense_net` / `income_net` / `refund_gross`）对 8 种 kind 的符号归属见 Transaction Kind Mapping。

**分期金额计算（ScheduledTransaction）**：每期金额固定的 MVP 决策与尾差计算规则见 ADR-0024「分期金额计算规则」（MVP 不支持每期金额不同、不支持 subscription 中途涨价）。

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

## Merchant（商户）

- **定义**：交易付款对象与收入来源的参考数据字典（如京东、红旗连锁、万科物业），交易（Transaction）以 `merchant_id` 引用，回答「这笔钱花给了谁、收入来自谁」；与分类正交——分类管「花在哪类」，商户管「花给谁」。
- **边界**：
  - 名字（`name`）全库唯一，是精确匹配复用（upsert）的前提。
  - 软删除（`is_deleted`）：删除后不再出现在补全/新建选择列表，历史引用保留照常显示；字典行硬删时外键置空（`ON DELETE SET NULL`），交易本身不删。
  - 改名即时生效：引用指向 id，不回刷历史交易行。
  - 与分类（Category）均可选、互不替代、互不联动：一笔交易可同时有分类与商户，也可都没有。
  - 可携带商户的 kind（行为层收口）：`expense` / `refund` / `income`；`transfer` / `buy` / `sell` / `dividend` / `split` 行为层拒绝。schema 层 `merchant_id` 允许 NULL、不设 kind 限制。refund 与账户/币种/分类同款继承语义：自动继承原支出商户、可修改。定时计划的分期/订阅形态每期生成交易时复制商户（见定时计划域 Counterparty 指针）。
  - 可选 `icon` / `color` 供报表与图表视觉辨识；随参考数据失效机制自动重拉（见参考数据与设置域 Reference Data）。
  - 建模、ADR-0024 决策点推翻与 schema 原地改（breaking change）决策见 ADR-0028。
- **别名**：不使用"交易对手"（已废弃的 Counterparty，见定时计划域指针词条）、"商家"（口语）、"收款方"（仅描述收入侧，以偏概全）。

## DefaultCurrency（默认币种）

- **定义**：用户在记账时首选的基准币种，用于展示本位币金额（`amount_native_cents`）。
- **边界**：MVP 阶段所有币种与默认币种汇率默认为 1:1；默认币种的选择影响 `formatAmount` 的 `decimal_places` 行为和报表的显示货币。
- **别名**：不使用"本位币"（偏会计术语）、"主币"（偏支付）。

## TransactionSearch（交易搜索）

- **定义**：用户通过文本关键字检索 `Transaction` 的功能入口，以独立视图呈现（侧边栏"搜索"入口，全局可达），MVP 阶段搜索范围仅限交易。
- **边界**：
  - 可搜索内容：交易备注（`note`）+ 转出账户名 + 商户名（issue #193 纳入；分类名、转入账户名不在搜索范围）。商户名口径：搜索时即时读取当前商户名（与账户名同款），软删商户的历史交易仍可搜、改名即刻生效。
  - 匹配语义：统一模糊搜索规格（ADR-0027）——词条之间 AND，每词条命中 = 原文连续子串（大小写不敏感）∨ 该字段拼音首字母串的子序列（如 `wy` 命中「万科物业」、`zsyh` 命中「招商银行」）。
  - 金额/日期筛选与关键字 AND 组合；结果按交易日期降序分页。
  - 搜索结果只读展示（复用交易列表的信息：日期、类型、账户、分类、金额、备注），不做增删改；需要操作时跳回交易列表。
  - 搜索词不持久化（与界面状态域 ViewState 边界一致）。
  - **时效性**：无索引、写入立即可搜；软删除即刻生效（ADR-0027）。
- **别名**：不使用"全局搜索"（范围并非全局，仅交易）、"全文搜索"（偏 FTS 技术含义）；"模糊搜索"可作为口语沿用（ADR-0004/0027 文档名），正式术语为"交易搜索"。

## 耗时日志（Timing Log）

- **定义**：对数据库执行操作按耗时记录的日志机制，用于建立性能基线、定位慢路径。
- **边界**：
  - 观测单位分两层：SQL 语句级（连接级 hook 全量覆盖，含启动迁移、自动备份、定时引擎）与命令/批次级（span 归因 + 批次汇总）。
  - 默认级别只落超阈值的慢查询；全量明细需 DEBUG 级别（`RUST_LOG=debug`）。
  - 日志遵循隐私约定：默认级别不记录金额等业务值（与 ADR-0006 的 INFO 级约定一致）。
- **别名**：不使用"时间日志"（含糊）、"性能日志"（偏优化手段，本机制只做观测）。

## 慢查询（Slow Query）

- **定义**：单条 SQL 执行耗时超过阈值的语句，阈值默认 100ms。
- **边界**：以 warn 级别记录；启动迁移、自动备份等合法慢路径也会命中，属预期信号而非故障；阈值可随观测数据调整。
- **别名**：不使用"慢 SQL"（口语沿用无妨，正式术语为"慢查询"）。

## 万分位分组（展示层口径）

- **定义**：所有数字展示的统一读数口径——整数部分从右向左每 4 位一组、半角逗号分隔（`1234567.89` → `123,4567.89`）；小数部分连续输出不分组、去掉全部小数尾零（`98.00` → `98`、`98.50` → `98.5`，小数位全空时连小数点一起去掉），负号恒在最前。全 App 固定采用，不做设置项。
- **边界**：
  - 仅作用于**展示层**字符串（金额 `formatAmount` 与数量 `formatQuantity` 共享同一核心助手）：存储仍为 `_cents` 整数分、输入解析与导出等机器可读路径不受影响、Rust 后端零改动。
  - ≤4 位整数天然不受影响；不同币种按各自小数位格式化后再对整数部分分组。
  - 数量列（股数/份额）走同一分组规则；带小数的份额仅整数部分分组。
- **别名**：不使用"千分位"（那是西文每 3 位一组的习惯，与本口径冲突）。
