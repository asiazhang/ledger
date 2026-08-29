# CONTEXT-MAP — 领域词汇表地图

Ledger 的领域词汇表按自然域拆分：本文件列出全部分域、各自位置与彼此关系。**动手前先读本地图，再按改动主题选读相关分域词汇表与相关 ADR**（决策型叙述与 MVP 取舍归 `docs/adr/`，词汇表条目只保留定义与 ADR 指针）。与代码行为冲突时，以代码为准并同步修正词汇表。

## 结构约定

- 全部分域文件**集中存放于 `docs/contexts/`**，不按源码目录散布——本仓库的域与目录结构不对齐（一个域横跨前后端），按目录放会误导；地图独自留在根目录，作为唯一入口。
- **术语全库唯一**：任一术语只在一份分域文件中定义，各分域文件间不复制定义。
- **跨域共享术语归核心交易域**：被多个域消费的概念（Transaction、Amount Model、Transaction Kind Mapping、Category、DefaultCurrency 等）只在核心交易域定义；其他域以「见核心交易域 X」引用，不复制定义。
- **新增术语进哪份文件**：按自然域归属放入对应分域；若它是被多域消费的共享概念，进核心交易域；单列小域只接纳体量小且与既有域边界清晰的独立概念（如物品域）。
- 一致性校验（地图与文件对应、术语唯一、导航一致）由独立检查脚本 `scripts/check-docs.sh` 守住，挂入 `scripts/check.sh` 质量门槛。

## 分域一览

| # | 分域 | 文件 | 条目主题 |
|---|------|------|----------|
| 1 | 核心交易 | [`docs/contexts/CONTEXT-core.md`](docs/contexts/CONTEXT-core.md) | Transaction、TransactionInput 装配器、InvolvingAccount、Amount Model、Transaction Kind Mapping、Category、Merchant、DefaultCurrency、TransactionSearch、万分位分组、耗时日志、慢查询 |
| 2 | 定时计划 | [`docs/contexts/CONTEXT-scheduled-plans.md`](docs/contexts/CONTEXT-scheduled-plans.md) | ScheduledTransaction 及三种业务形态（分期/订阅/定时转账）、Occurrence、Plan Lifecycle、Timing、Counterparty（废弃→Merchant 指针）、Recurrence Rule、Failure Policy |
| 3 | 投资域 | [`docs/contexts/CONTEXT-investment.md`](docs/contexts/CONTEXT-investment.md) | Instrument、MarketPrice、PriceHistory、FxRateHistory、PortfolioValueTrend、Holding、NetWorth、Investment、InvestedInstrument、InstrumentSync、HoldingPriceSync |
| 4 | AI 导入 | [`docs/contexts/CONTEXT-ai-import.md`](docs/contexts/CONTEXT-ai-import.md) | AI API、AIReadbackVerification、AICleanupDeletion、AICleanupModify、ImportDedup、IdempotencyKey、BlackHoleAccount、AIPrompt |
| 5 | 参考数据与设置 | [`docs/contexts/CONTEXT-reference-settings.md`](docs/contexts/CONTEXT-reference-settings.md) | Reference Data、Appearance、AppSettings、轻量设置项、DataLocation |
| 6 | 备份与数据文件 | [`docs/contexts/CONTEXT-backup-datafiles.md`](docs/contexts/CONTEXT-backup-datafiles.md) | Backup、Restore、RestoreSafetyBackup、BackupDirectory、BackupRetentionLimit、BackupPruning、ManagedBackup、ManualBackup、BackupTrigger、AutoBackup、DirtyMarker |
| 7 | 界面状态与交互 | [`docs/contexts/CONTEXT-ui-interaction.md`](docs/contexts/CONTEXT-ui-interaction.md) | WindowState、ViewState、ViewShortcut、CreateShortcut、Overlay Suppression、ESC 键语义、原生右键菜单、拼音可搜下拉 |
| 8 | 物品 | [`docs/contexts/CONTEXT-item.md`](docs/contexts/CONTEXT-item.md) | Item、DailyUsageCost、source_transaction_id、创建语义 |
| 9 | 预算 | [`docs/contexts/CONTEXT-budget.md`](docs/contexts/CONTEXT-budget.md) | Budget、BudgetPeriod、BudgetProgress（永久滚动，ADR-0029） |

## 域间关系

关系即归属逻辑——一个术语放哪个域，由「谁定义它、谁消费它」决定：

- **核心交易是被所有域消费的底座**。Transaction 与 Amount Model（raw/native 分离 + kind→度量符号矩阵）定义「一笔资金变动长什么样」，Category / Merchant / DefaultCurrency 是它依赖的字典与折算基准；定时计划、投资、AI 导入、物品四域产出的最终都是核心域的交易流水（或以它为对账基准），所以这些共享概念只在核心域定义。
- **商户（Merchant）→ 核心交易定义、三域消费**：商户是继 Category / DefaultCurrency 之后核心交易域的又一共享参考字典（ADR-0028）——核心交易的 `expense` / `refund` / `income` 流水以 `merchant_id` 引用它；定时计划的分期/订阅形态挂商户并随期次复制到流水（Counterparty 自由文本已废弃，词条改为指针）；AI 导入自动识别商户、精确匹配已有名字复用或即建。
- **定时计划 → 核心交易**：ScheduledTransaction 及其三种业务形态（InstallmentPlan / Subscription / ScheduledTransfer）是生成核心域 Transaction 的规则与模板，每期触发落一条流水（Occurrence）；生命周期、周期规则、失败策略等 MVP 决策在 ADR-0024；分期/订阅可挂核心域商户（ADR-0028）。
- **投资域 → 核心交易**：buy/sell 首先是核心域 Transaction 的 kind，Investment 是它背后的持仓/盈亏载体；市值与净资产折算消费核心域 DefaultCurrency，历史折算经本域 FxRateHistory。
- **AI 导入 → 核心交易 + 参考数据**：AI 经本域 AI API 幂等写入核心域 Transaction 与参考数据（账户/分类/币种/商户），ImportDedup / IdempotencyKey 是写入侧的去重契约，读回验证（AIReadbackVerification）按核心域 InvolvingAccount 与余额口径对账；BlackHoleAccount 是参考数据中的特殊账户，因由导入流程预置与消费而归本域。
- **参考数据与设置 → 被核心交易引用、与备份相邻**：Reference Data（账户/分类/币种字典）被核心域 Transaction 以外键引用；AppSettings 是后端配置与运行时状态的权威落点，备份域的调度状态（AutoBackup / DirtyMarker）存于其中——这是它与备份域的相邻点；DataLocation 因「建连前必须可读」成为库外引导配置的唯一例外。
- **备份与数据文件 ↔ 各域相邻而不交叉**：Backup/Restore 是文件级整库快照通道，与 AI 导入（语义级写入）、行情同步（投资域）互不交叉；备份不迁移界面状态（界面域）与设备偏好（参考设置域 / 核心域 DefaultCurrency）。
- **界面状态与交互 → 只读消费各域**：WindowState / ViewState / 快捷键 / 弹层抑制是纯界面层概念，不持业务数据；搜索（核心域 TransactionSearch）复用交易列表信息但搜索词不持久化（与 ViewState 边界一致）；实体下拉的拼音过滤（拼音可搜下拉）与全局搜索共用统一模糊搜索语义规格（ADR-0027）。
- **物品（单列小域）→ 挂靠核心交易**：Item 是与参考数据、交易流水、投资标的并列的独立领域概念，自包含总成本、不进字典；创建**必须**关联一笔核心域 `expense` Transaction（`source_transaction_id` 必填、唯一，仅用于带出成本/日期与溯源），**不建立反向引用**，**无交易物品不可录入**；创建唯一入口 = 交易右键 + 确认弹窗（ADR-0025）。
- **预算（单列小域）→ 核心交易**：Budget 是对核心域支出分类（Category）的持续性支出上限，**永久滚动**——进度窗口永远是当前自然周期、随时间自动滚动，不持久化窗口状态（ADR-0029）；进度 spent 消费核心域 Amount Model 的 ExpenseNet 度量（与报表分类净值同口径），不另造第二个口径；`start_date` 为已发布 schema 的冻结残留，不参与计算。
