# CONTEXT-MAP — 领域词汇表地图

Ledger 的领域词汇表按自然域拆分：本文件列出全部分域、各自位置与彼此关系。**动手前先读本地图，再按改动主题选读相关分域词汇表与相关 ADR**（决策型叙述与 MVP 取舍归 `docs/adr/`，词汇表条目只保留定义与 ADR 指针）。与代码行为冲突时，以代码为准并同步修正词汇表。

## 结构约定

- 全部分域文件**集中存放于 `docs/contexts/`**，不按源码目录散布——一个自然域横跨前端组件/状态与后端域引擎/壳层（源码按技术栈与端分置于 `src/` 与 `src-tauri/src/`），词汇表按源码目录散布会割裂域视角；地图独自留在根目录，作为唯一入口。
- **术语全库唯一**：任一术语只在一份分域文件中定义，各分域文件间不复制定义。
- **跨域共享术语归核心交易域**：被多个域消费的概念（Transaction、Amount Model、Transaction Kind Mapping、Category、DefaultCurrency 等）只在核心交易域定义；其他域以「见核心交易域 X」引用，不复制定义。
- **新增术语进哪份文件**：按自然域归属放入对应分域；若它是被多域消费的共享概念，进核心交易域；单列小域只接纳体量小且与既有域边界清晰的独立概念（如物品域）。
- **代码可查事实不进文档（三层标尺）**：分域词汇表与模型文档按一条标尺取舍内容——**甲类删**：实现坐标（文件路径、函数名、参数名、字段清单、DDL、正则、公式等能从代码直接查出的事实），schema 字段以 migration、行为以代码为唯一事实来源；**乙类留**：作为术语本体的标识符（表名、视图名、事件名、信号名、接缝名、列名），只作专名出现、不复述结构；**丙类留**：闭集性、口径归属、边界、动机等纯语义。导航职责收口 AGENTS.md，语义职责收口词汇表，二者不互相复述。
- 一致性校验（地图与文件对应、术语唯一、导航一致、代码坐标）由独立检查脚本 `scripts/check-docs.sh` 守住，挂入 `scripts/check.sh` 质量门槛。

## 分域一览

| # | 分域 | 文件 | 条目主题 |
|---|------|------|----------|
| 1 | 核心交易 | [`docs/contexts/CONTEXT-core.md`](docs/contexts/CONTEXT-core.md) | Transaction、TransactionInput 装配器、InvolvingAccount、Amount Model、Transaction Kind Mapping、债权债务往来（借出/借入）、Category、Merchant、DefaultCurrency、TransactionSearch、错误码、数字分组、耗时日志、慢查询 |
| 2 | 定时计划 | [`docs/contexts/CONTEXT-scheduled-plans.md`](docs/contexts/CONTEXT-scheduled-plans.md) | ScheduledTransaction 及三种业务形态（分期/订阅/定时转账）、Occurrence、Plan Lifecycle、Timing、SubscriptionSpend、Counterparty（废弃→Merchant 指针）、Recurrence Rule、Failure Policy、Auto Execution（自动执行·追补） |
| 3 | 投资域 | [`docs/contexts/CONTEXT-investment.md`](docs/contexts/CONTEXT-investment.md) | Instrument、MarketPrice、PriceHistory、FxRateHistory、PortfolioValueTrend、Holding、NetWorth、InvestableAssets、FinancialFreedom、时点持仓、Investment、TransactionTrade、InvestedInstrument、自建标的、手动报价、价格刻度、InstrumentSync、HoldingPriceSync、价格失效信号 |
| 4 | AI 导入 | [`docs/contexts/CONTEXT-ai-import.md`](docs/contexts/CONTEXT-ai-import.md) | AI API、AIReadbackVerification、AICleanupDeletion、AICleanupModify、ImportDedup、IdempotencyKey、BlackHoleAccount、AIPrompt |
| 5 | 参考数据与设置 | [`docs/contexts/CONTEXT-reference-settings.md`](docs/contexts/CONTEXT-reference-settings.md) | Reference Data、BalanceAdjustment、Appearance、界面语言、应用名称、AppSettings、轻量设置项、日志等级、金额隐私模式、DataLocation |
| 6 | 备份与数据文件 | [`docs/contexts/CONTEXT-backup-datafiles.md`](docs/contexts/CONTEXT-backup-datafiles.md) | Backup、Restore、RestoreSafetyBackup、BackupDirectory、BackupRetentionLimit、BackupPruning、ManagedBackup、ManualBackup、BackupTrigger、AutoBackup、DirtyMarker、加密模式、主口令、口令强度、解锁、自动解锁、启动失败恢复、原位重引导 |
| 7 | 界面状态与交互 | [`docs/contexts/CONTEXT-ui-interaction.md`](docs/contexts/CONTEXT-ui-interaction.md) | TransactionFilter、数据期间边界、时间范围快捷选择、TransactionModalState（交易弹窗编排）、ModalIntent（弹窗意图编排）、RowContextMenu（行右键菜单编排）、Loadable（异步任务）、GlobalBusyBar（全局忙碌条）、ScheduledPlanList（计划清单）、WindowState、ViewState、会话内保留、ViewShortcut、侧栏分组、组内收纳、CreateShortcut、Overlay Suppression、弹层关闭语义、对话框排版（Dialog Layout）、ESC 键语义、原生右键菜单、界面文本不可选、拼音可搜下拉、报表年份筛选（已退役→时间范围快捷选择）、分类下钻、商户排行下钻、来源列、实体定位参数（focus 参数） |
| 8 | 物品 | [`docs/contexts/CONTEXT-item.md`](docs/contexts/CONTEXT-item.md) | Item、DailyUsageCost、source_transaction_id、创建语义 |
| 9 | 预算 | [`docs/contexts/CONTEXT-budget.md`](docs/contexts/CONTEXT-budget.md) | Budget（可挂任意层级支出分类、彼此独立，ADR-0052）、BudgetPeriod、BudgetProgress（永久滚动，ADR-0029；父含子、子只算自身）、AnnualBudgetTotal |
| 10 | 保险 | [`docs/contexts/CONTEXT-insurance.md`](docs/contexts/CONTEXT-insurance.md) | Policy（保单）、Insurer（保险公司）、Premium（保费）、PolicyInflow（保单现金流入）、PolicyReference（保单引用）、保单视角统计（ADR-0051、ADR-0082） |
| 11 | 实物资产 | [`docs/contexts/CONTEXT-physical-asset.md`](docs/contexts/CONTEXT-physical-asset.md) | PhysicalAsset（实物资产）、Valuation（估值）、ValuationHistory（估值历史）、Disposal（处置）（ADR-0064） |
| 12 | 测试基础设施 | [`docs/contexts/CONTEXT-testing.md`](docs/contexts/CONTEXT-testing.md) | invoke 测试接缝、defaults 表、overrides 表、未命中报错、参考数据预热、清理四件套、消息替身稳定实例、目录级测试薄壳、行为等价判据（ADR-0085）；测试世界、步骤输入工厂、步骤动词、快照分组、公开写入口（测试侧）（ADR-0086）；测试三层与权威层、壳三件套、接线证明、错误码契约代表（ADR-0087） |

## 域间关系

关系即归属逻辑——一个术语放哪个域，由「谁定义它、谁消费它」决定：

- **核心交易是被所有域消费的底座**。Transaction 与 Amount Model（raw/native 分离 + kind→度量符号矩阵）定义「一笔资金变动长什么样」，Category / Merchant / DefaultCurrency 是它依赖的字典与折算基准；定时计划、投资、AI 导入、物品四域产出的最终都是核心域的交易流水（或以它为对账基准），所以这些共享概念只在核心域定义。
- **商户（Merchant）→ 核心交易定义、三域消费**：商户是继 Category / DefaultCurrency 之后核心交易域的又一共享参考字典（ADR-0028）——核心交易的 `expense` / `refund` / `income` 流水以 `merchant_id` 引用它；定时计划的分期/订阅形态挂商户并随期次复制到流水（保单缴费协议除外：保费流水不挂商户，ADR-0082；Counterparty 自由文本已废弃，词条改为指针）；AI 导入自动识别商户、精确匹配已有名字复用或即建。
- **定时计划 → 核心交易**：ScheduledTransaction 及其三种业务形态（InstallmentPlan / Subscription / ScheduledTransfer）是生成核心域 Transaction 的规则与模板，每期触发落一条流水（Occurrence）；生命周期、周期规则、失败策略等 MVP 决策在 ADR-0024；分期/订阅可挂核心域商户（ADR-0028）。
- **投资域 → 核心交易**：buy/sell 首先是核心域 Transaction 的 kind（场外基金申赎同构复用，ADR-0038），Investment 是它背后的持仓/盈亏载体；市值与净资产折算消费核心域 DefaultCurrency，历史折算经本域 FxRateHistory。财务自由度（FinancialFreedom）以本域可投资资产（InvestableAssets）为分子、跨域消费预算域年度预算总额（AnnualBudgetTotal）为分母（ADR-0048）。
- **AI 导入 → 核心交易 + 参考数据 + 投资域**：AI 经本域 AI API 幂等写入核心域 Transaction 与参考数据（账户/分类/币种/商户）及投资域标的（buy/sell 迁移先经标的搜索/幂等创建解析为标的 id；场外基金按真实代码建行，ADR-0039），ImportDedup / IdempotencyKey 是写入侧的去重契约，读回验证（AIReadbackVerification）按核心域 InvolvingAccount 与余额口径对账；BlackHoleAccount 是参考数据中的特殊账户，因由导入流程预置与消费而归本域。
- **参考数据与设置 → 被核心交易引用、与备份相邻**：Reference Data（账户/分类/币种字典）被核心域 Transaction 以外键引用；AppSettings 是后端配置与运行时状态的权威落点，备份域的调度状态（AutoBackup / DirtyMarker）存于其中——这是它与备份域的相邻点；DataLocation 因「建连前必须可读」成为库外引导配置的唯一例外。
- **备份与数据文件 ↔ 各域相邻而不交叉**：Backup/Restore 是文件级整库快照通道，与 AI 导入（语义级写入）、行情同步（投资域）互不交叉；备份不迁移界面状态（界面域）与设备偏好（参考设置域 / 核心域 DefaultCurrency）。
- **界面状态与交互 → 只读消费各域**：WindowState / ViewState / 快捷键 / 弹层抑制 / 弹层关闭语义是纯界面层概念，不持业务数据；组内收纳（每组「更多」聚合页与用户移入移出，ADR-0063，取代 ADR-0055 全局单一收容器）同为纯界面层信息架构——被收视图整体迁入为所在组的页签，只动导航位置、不改域语义与功能；TransactionFilter（交易列表过滤模块）消费参考数据就绪状态（参考设置域 Reference Data）与 URL 下钻参数，筛选不持久化（与 ViewState 边界一致，ADR-0030）；ScheduledPlanList（计划清单）把定时计划域 ScheduledTransaction 三形态的清单编排收为深模块、与计划表单接缝构成计划域前端双接缝，生命周期变更仍走定时计划域命令（ADR-0041）；来源列把交易的发起来源实体（计划三形态/保单/物品/标的，均反查既有指针、不新增数据级反向引用）显示为可点击链接，经实体定位参数精确落地，落点尊重组内收纳；搜索（核心域 TransactionSearch）复用交易列表信息但搜索词不持久化（与 ViewState 边界一致）；实体下拉的拼音过滤（拼音可搜下拉）与交易搜索共用统一模糊搜索语义规格（唯一定义点见核心域 TransactionSearch 词条，ADR-0027）。
- **物品（单列小域）→ 挂靠核心交易**：Item 是与参考数据、交易流水、投资标的并列的独立领域概念，自包含总成本、不进字典；创建**必须**关联一笔核心域 `expense` Transaction（`source_transaction_id` 必填、唯一，仅用于带出成本/日期与溯源），**不建立反向引用**，**无交易物品不可录入**；创建唯一入口 = 交易右键 + 确认弹窗（ADR-0025）。
- **预算（单列小域）→ 核心交易**：Budget 是对核心域支出分类（Category）的持续性支出上限，可挂任意层级支出分类且**彼此独立**——父子预算并存不扣减、不互斥，父管总量、子管细分，同一笔支出有意计入两条预算（ADR-0052）；**永久滚动**——进度窗口永远是当前自然周期、随时间自动滚动，不持久化窗口状态（ADR-0029）；进度 spent 消费核心域 Amount Model 的 ExpenseNet 度量（与报表分类净值同口径），不另造第二个口径；`start_date` 为已发布 schema 的冻结残留，不参与计算。年度预算总额（AnnualBudgetTotal）是预算口径的跨域派生聚合（无窗口年化），供投资域财务自由度作分母，不改变 BudgetProgress 的输出边界。
- **保险（单列小域）→ 核心交易 + 定时计划**：Policy 是消费型保险合同的静态档案，保险公司为保险域自有独立字典（Insurer，不复用商户，ADR-0082 推翻 ADR-0051 决策 7）；缴费协议复用定时计划域订阅形态（Subscription）生成核心域 `expense` 保费流水（不挂商户，付款对象语义由保单保司承担），理赔款记核心域 `income`（非 refund，保住保费支出口径不失真）；保费/现金流入流水经 PolicyReference 可选直挂保单，保单可无协议独立建档、软删后引用保留，保单视角统计按流水忠实推导、不落库；AI 导入（AI 导入域 AI API）不识别保单（MVP 边界）；储蓄型现金价值资产化缓行、不在本域范围。
- **测试基础设施 → 替身消费各域命令契约**：invoke 测试接缝布线的是各域 IPC 命令的替身应答（命令名与语义归各业务分域），参考数据预热的规范夹具是参考设置域 Reference Data 的测试投影；本域只定义布线、清理与行为等价判据语义（ADR-0085），不持业务语义。
- **测试基础设施 → BDD 步骤层消费各域公开写入口**：步骤动词经域层/行为层公开函数造数与动作，业务不变量由产品代码保证，裸 SQL 限于无公开入口的库内状态直置并显式登记（ADR-0086）；被测实体与命令的语义归各业务分域，本域只定义测试层的构造、写入与分组纪律。
- **实物资产（单列小域）→ 核心交易（只消费不产流水）**：PhysicalAsset 是大件实物的估值档案，与物品域 Item 按「要不要跟踪市值」互斥分家（物品摊成本、资产记市值）；估值全手动且每次追加估值历史行（当前值 = 最新一条），金额一律整数分、折本位币走核心域 Amount 接缝（当期汇率，缺汇率错误上抛）；币种复用核心域币种字典；在持估值计入净资产第三腿（ADR-0064），已处置/软删不计、可投资资产（财务自由度分子）不受影响；入口收纳「更多」页签（ADR-0055）；无交易流水产出，纯 IPC 面，AI 导入不识别（MVP 边界）。
