# CONTEXT-MAP — 领域词汇表地图

Ledger 的领域词汇表按自然域拆分：本文件列出全部分域、各自位置与彼此关系。**动手前先读本地图，再按改动主题选读相关分域词汇表与相关 ADR**（决策型叙述与 MVP 取舍归 `docs/adr/`，词汇表条目只保留定义与 ADR 指针）。与代码行为冲突时，以代码为准并同步修正词汇表。

## 结构约定

- 全部分域文件**集中存放于 `docs/contexts/`**，不按源码目录散布——一个自然域横跨前端组件/状态与后端域引擎/壳层（源码按技术栈与端分置于 `src/`、`src-tauri/src/`，前端子包落 `packages/*`，issue #1149），词汇表按源码目录散布会割裂域视角；地图独自留在根目录，作为唯一入口。
- **术语全库唯一**：任一术语只在一份分域文件中定义，各分域文件间不复制定义。
- **跨域共享术语归核心交易域**：被多个域消费的概念（Transaction、Amount Model、Transaction Kind Mapping、Category、DefaultCurrency 等）只在核心交易域定义；其他域以「见核心交易域 X」引用，不复制定义。
- **新增术语进哪份文件**：按自然域归属放入对应分域；若它是被多域消费的共享概念，进核心交易域；单列小域只接纳体量小且与既有域边界清晰的独立概念（如物品域）。
- **代码可查事实不进文档（三层标尺 + ADR 坐标收敛）**：分域词汇表与模型文档按一条标尺取舍内容——**甲类删**：实现坐标（文件路径、函数名、参数名、字段清单、DDL、正则、公式等能从代码直接查出的事实），schema 字段以 migration、行为以代码为唯一事实来源；**乙类留**：作为术语本体的标识符（表名、视图名、事件名、信号名、接缝名、列名），只作专名出现、不复述结构；**丙类留**：闭集性、口径归属、边界、动机等纯语义。导航职责收口 AGENTS.md，语义职责收口词汇表，二者不互相复述。ADR 按同一取向收敛坐标——现役落点保留域、crate 与模块/函数专名，不写文件路径与行号；历史对照可在注记中保留旧坐标原文并加注现役落点。已存在但未失真的历史坐标不做一次性迁移，漂移命中时随修订收敛。ADR 不纳入脚本⑤扫描，扫描范围仍限分域词汇表与模型文档。
- **前端壳内按域归位（issue #1159 / ADR-0118 决策 4/5）**：壳内域源码按自然域分目录，与后端业务域 crate 多数同名对应（同名例外与无独立目录的域见本条内注），域目录即「哪一域对应哪个目录」的单一清单——`src/accounts/`、`src/backup/`（备份与数据文件域的前端面）、`src/categories/`、`src/dashboard/`（首页卡片数据层）、`src/investment/`、`src/item/`、`src/merchants/`、`src/physical-asset/`、`src/policy/`（含保司字典）、`src/reports/`、`src/scheduled/`、`src/settings/`（设置页签、功能开关与账本侧栏入口，对应参考数据与设置域的后端设置/账本面，币种与本位币前端面同住此目录）、`src/transaction/`，每域一目录收拢本域组件、composable、store 与纯逻辑；预算域前端仅预算视图、无独立目录，行情同步（ledger-market-sync）与多端同步（ledger-sync-engine）的前端面分别住在 investment 与 settings。**跨域件落点**：不属任何单一域的壳件留在 `src/components/`、`src/composables/`、`src/stores/` 三根目录（应用壳与界面交互件；stores 根仅存 app / reference / render-errors / sidebar-order 四件）；通用件住 `packages/*`（`@ledger/ui-kit` 等，成员闭集与深模块成包裁定见 ADR-0118）；视图（`src/views/`）、路由（`src/router/`）与应用入口留壳不随域搬。
- 一致性校验（地图与文件对应、术语唯一、导航一致、代码坐标）由独立检查脚本 `scripts/check-docs.sh` 守住，挂入 `scripts/check.sh` 质量门槛。

## 分域一览

| # | 分域 | 文件 | 条目主题 |
|---|------|------|----------|
| 1 | 核心交易 | [`docs/contexts/CONTEXT-core.md`](docs/contexts/CONTEXT-core.md) | Transaction、写入协议（Write Protocol）、TransactionInput 装配器、InvolvingAccount、出资账户、Amount Model、Transaction Kind Mapping、债权债务往来（借出/借入）、Category、Merchant、DefaultCurrency、TransactionSearch、错误码、数字分组、耗时日志、慢查询、失效信号（家族术语） |
| 2 | 定时计划 | [`docs/contexts/CONTEXT-scheduled-plans.md`](docs/contexts/CONTEXT-scheduled-plans.md) | ScheduledTransaction 及三种业务形态（分期/订阅/定时转账）、Occurrence、Plan Lifecycle、Timing、SubscriptionSpend、Counterparty（废弃→Merchant 指针）、Recurrence Rule、Failure Policy、Auto Execution（自动执行·追补） |
| 3 | 投资域 | [`docs/contexts/CONTEXT-investment.md`](docs/contexts/CONTEXT-investment.md) | Instrument、MarketPrice、PriceHistory、价格历史后台补全（PriceHistoryBackfill）、价格通道（PriceChannel）、价格恒定标的（Constant-Price Instrument）、行情批量取数（BulkQuoteFetch）、FxRateHistory、PortfolioValueTrend、Holding、NetWorth、InvestableAssets、FinancialFreedom、跨账本投资汇总（CrossBookInvestmentSummary）、投资概览（InvestmentOverview）、时点持仓、Investment、Unwind（持仓副作用撤销）、基金转换（Conversion）、份额调整（Split）、现金分红（Dividend）、红利再投（DividendReinvestment）、TransactionTrade、TransactionConvert、InvestedInstrument、已实现盈亏（RealizedPnl）、已实现收益（RealizedGain）、年度收益（AnnualReturn）、累计收益（CumulativePnl）、资金加权收益率（MoneyWeightedReturn）、期初存量（OpeningBalance）、自建标的、手动报价、行情接入（QuoteAdoption）、价格刻度、份额容差（QuantityTolerance）、InstrumentSync（已退役→按代码查询/创建）、InstrumentInfoSync（前名 HoldingPriceSync）、价格过期提示（PriceStalenessPrompt）、价格失效信号 |
| 4 | AI 导入 | [`docs/contexts/CONTEXT-ai-import.md`](docs/contexts/CONTEXT-ai-import.md) | AI API、AI 记账、AIReadbackVerification、AICleanupDeletion、AICleanupModify、ImportDedup、IdempotencyKey、BlackHoleAccount、知识索引、分域知识节、AIPrompt |
| 5 | 参考数据与设置 | [`docs/contexts/CONTEXT-reference-settings.md`](docs/contexts/CONTEXT-reference-settings.md) | Reference Data、ExchangeRate（当期汇率）、信用卡档案（Credit Terms）、BalanceAdjustment、Appearance、界面语言、应用名称、AppSettings、轻量设置项、功能开关、LedgerLevelSetting、日志等级、金额隐私模式、账本（Book）、DataLocation |
| 6 | 备份与数据文件 | [`docs/contexts/CONTEXT-backup-datafiles.md`](docs/contexts/CONTEXT-backup-datafiles.md) | Backup、Restore、RestoreSafetyBackup、BackupDirectory、BackupRetentionLimit、BackupPruning、ManagedBackup、ManualBackup、BackupTrigger、AutoBackup、DirtyMarker、加密模式、形态转换、主口令、口令强度、解锁、自动解锁、启动失败恢复、原位重引导 |
| 7 | 界面状态与交互 | [`docs/contexts/CONTEXT-ui-interaction.md`](docs/contexts/CONTEXT-ui-interaction.md) | TransactionFilter、客户端切片分页、数据期间边界、时间范围快捷选择、TransactionModalState（交易弹窗编排）、ModalIntent（弹窗意图编排）、RowContextMenu（行右键菜单编排）、Loadable（异步任务）、GlobalBusyBar（全局忙碌条）、ScheduledPlanList（计划清单）、形态转换编排（Mode Transition Orchestration）、WindowState、ViewState、会话内保留、ViewShortcut、侧栏分组、组内收纳、CreateShortcut、Overlay Suppression、弹层关闭语义、对话框排版（Dialog Layout）、ESC 键语义、原生右键菜单、界面文本不可选、拼音可搜下拉、报表年份筛选（已退役→时间范围快捷选择）、分类下钻、商户排行下钻、持仓下钻、来源列、实体定位参数（focus 参数）、窗口分级（Window Tier）、输入轴（Input Mode）、导航抽屉（Navigation Drawer）、记一笔悬浮按钮（Create FAB）、系统返回语义（System Back）、桌面专属管理动作（Desktop-Only Management）、样式方案（Styling Scheme）、Design Tokens |
| 8 | 物品 | [`docs/contexts/CONTEXT-item.md`](docs/contexts/CONTEXT-item.md) | Item、DailyUsageCost、source_transaction_id、创建语义 |
| 9 | 预算 | [`docs/contexts/CONTEXT-budget.md`](docs/contexts/CONTEXT-budget.md) | Budget（可挂任意层级支出分类、彼此独立，ADR-0052）、BudgetPeriod、BudgetProgress（永久滚动，ADR-0029；父含子、子只算自身）、AnnualBudgetTotal |
| 10 | 保险 | [`docs/contexts/CONTEXT-insurance.md`](docs/contexts/CONTEXT-insurance.md) | Policy（保单）、Insurer（保险公司）、Premium（保费）、PolicyInflow（保单现金流入）、PolicyReference（保单引用）、保单视角统计（ADR-0051、ADR-0082） |
| 11 | 实物资产 | [`docs/contexts/CONTEXT-physical-asset.md`](docs/contexts/CONTEXT-physical-asset.md) | PhysicalAsset（实物资产）、Valuation（估值）、ValuationHistory（估值历史）、Disposal（处置）（ADR-0064） |
| 12 | 测试基础设施 | [`docs/contexts/CONTEXT-testing.md`](docs/contexts/CONTEXT-testing.md) | invoke 测试接缝、defaults 表、overrides 表、未命中报错、参考数据预热、清理四件套、消息替身稳定实例、目录级测试薄壳、行为等价判据（ADR-0085）；测试世界、步骤输入工厂、步骤动词、快照分组、公开写入口（测试侧）（ADR-0086）；测试三层与权威层、壳三件套、接线证明、错误码契约代表（ADR-0087）；断言强度、双断言、矩阵、存在性断言、组件测试数据工厂 |
| 13 | 多端同步 | [`docs/contexts/CONTEXT-sync.md`](docs/contexts/CONTEXT-sync.md) | Sync、Transport、OpLog、DomainCommand、Replay、DeviceId、TotalOrder、OccurrenceKey、ParkedOp、Checkpoint、SyncEnvelope、SyncBoundary（ADR-0091） |
| 14 | 应用更新 | [`docs/contexts/CONTEXT-app-update.md`](docs/contexts/CONTEXT-app-update.md) | 自动更新（Auto Update）、更新检查、更新清单、更新工件、更新签名（ADR-0124） |

## 域间关系

关系即归属逻辑——一个术语放哪个域，由「谁定义它、谁消费它」决定。各条只保留归属箭头、归属理由与跨域边，词条定义与边界细节归分域文件，此处不复述：

- **核心交易是被所有域消费的底座**。Transaction 与 Amount Model 定义「一笔资金变动长什么样」，Category / Merchant / DefaultCurrency 是它依赖的字典与折算基准——这些共享概念只在核心域定义；其余各域的写入最终落为核心域交易流水（或以它为对账基准）。跨域共享的失效信号家族术语同样定义于此，成员按域限定（参考失效信号 / 价格失效信号 / 备份信号）。
- **商户（Merchant）→ 核心交易定义、三域消费**：继 Category / DefaultCurrency 之后核心交易域的又一共享参考字典（ADR-0028）。消费方：核心交易流水以 `merchant_id` 引用（transfer 为借贷关联指针，ADR-0092）、定时计划分期/订阅挂商户随期次复制（保单缴费除外，ADR-0082；Counterparty 已废弃为指针词条）、AI 导入识别复用或即建。
- **定时计划 → 核心交易**：ScheduledTransaction 三种业务形态（InstallmentPlan / Subscription / ScheduledTransfer）是生成核心域 Transaction 的模板，每期触发落一条流水（Occurrence）；MVP 决策在 ADR-0024；分期/订阅挂核心域商户（见商户条）。
- **投资域 → 核心交易 + 预算**：buy/sell 与现金分红（Dividend）消费核心域 kind（ADR-0038、ADR-0109），资金出入经核心域出资账户归因（ADR-0096），折算消费核心域 DefaultCurrency（历史折算经本域 FxRateHistory）；红利再投不新增 kind，词条见投资域。财务自由度（FinancialFreedom）以本域可投资资产（InvestableAssets）为分子、跨域消费预算域年度预算总额（AnnualBudgetTotal）为分母（ADR-0048）；跨账本投资汇总（CrossBookInvestmentSummary）是账本「不跨本汇总」边界下本域的唯一跨本例外（ADR-0114）。
- **AI 导入 → 核心交易 + 参考数据 + 投资域**：AI 经本域 AI API 幂等写入核心域 Transaction、参考数据字典与投资域标的（标的按真实代码解析为 id，ADR-0081/ADR-0039）；ImportDedup / IdempotencyKey 是写入侧的去重契约，读回验证（AIReadbackVerification）按核心域 InvolvingAccount 与余额口径对账；BlackHoleAccount 因由导入流程预置与消费而归本域。会话形态与知识分级（ADR-0110）的语义见 AI 导入域词条。
- **参考数据与设置 → 被核心交易引用、与备份相邻**：Reference Data（账户/分类/币种字典）被核心域 Transaction 以外键引用；汇率（ExchangeRate）归币种参考数据（ADR-0059），被核心域流水折算的当期路径与投资域市值折算消费（历史期与流水折算的取数走投资域 FxRateHistory）；AppSettings 是后端配置与运行时状态的权威落点，备份域的调度状态（AutoBackup / DirtyMarker）存于其中——这是它与备份域的相邻点；账本（Book）与账本注册表归本域——多本账清单与活动指针是「建连前必须可读」的引导配置，落库外引导文件（ADR-0089 修订 ADR-0018，实现分层见 ADR-0111）；信用卡档案归本域且不进任何金额口径（ADR-0119）。DataLocation 的收窄语义见参考设置域词条。
- **备份与数据文件 ↔ 各域相邻而不交叉**：Backup/Restore 是文件级整库快照通道，与 AI 导入（语义级写入）、行情同步（投资域）互不交叉；备份不迁移界面状态与设备偏好。按账本分域的产物命名、清理与解锁缓存细节见备份域词条。
- **界面状态与交互 → 只读消费各域**：WindowState / ViewState / 快捷键 / 弹层抑制 / 弹层关闭语义 / 组内收纳（ADR-0063）是纯界面层概念，不持业务数据；各模块的跨域边一律只读——TransactionFilter 消费参考数据就绪状态与 URL 下钻参数（筛选不持久化，ADR-0030）、ScheduledPlanList 经定时计划域命令变更生命周期（ADR-0041）、来源列反查既有指针不新增数据级反向引用、拼音可搜下拉共用核心域 TransactionSearch 的统一模糊搜索规格（ADR-0027）、功能开关（见参考设置域）只收窄入口不改写收纳清单（ADR-0116）。
- **物品（单列小域）→ 挂靠核心交易**：Item 是与参考数据、交易流水、投资标的并列的独立领域概念，自包含总成本、不进字典；唯一锚点是创建必挂一笔核心域 `expense` 交易（溯源指针、无反向引用），创建语义与唯一入口见物品域词条（ADR-0025）。
- **预算（单列小域）→ 核心交易**：Budget 挂核心域支出分类（Category）设定支出上限，词条定义见预算域文件；跨域边一条——年度预算总额（AnnualBudgetTotal）供投资域财务自由度作分母（ADR-0048）。
- **保险（单列小域）→ 核心交易 + 定时计划**：Policy 是消费型保险合同的静态档案；缴费复用定时计划域订阅形态（Subscription）生成核心域 `expense` 保费流水（不挂商户），理赔款记核心域 `income`；保司为本域自有独立字典（Insurer，不复用商户，ADR-0082）。范围与 MVP 边界见保险域词条。
- **测试基础设施 → 替身与步骤层消费各域**：invoke 测试接缝布线各域 IPC 命令的替身应答（参考数据预热是参考设置域 Reference Data 的测试投影），BDD 步骤动词经各域公开写入口造数——命令与被测实体的业务语义归各业务分域，本域只定义布线、清理、构造与行为等价判据（ADR-0085、ADR-0086），不持业务语义。
- **多端同步 → 经写入接缝重放各域语义命令**：同步域不定义业务语义，op 载荷就是各域既有写入命令，重放是行为编排之外的第 N 写入入口（ADR-0091）；与备份域相邻——SyncEnvelope 复用加密模式 / 主口令，但备份 ≠ 同步（快照还原 vs 增量合并）。期次去重与同步边界细节见同步域词条。
- **实物资产（单列小域）→ 核心交易（只消费不产流水）**：PhysicalAsset 是大件实物的估值档案，与物品域 Item 按「要不要跟踪市值」互斥分家；金额折算走核心域 Amount 接缝、币种复用核心域字典。估值机制、净资产口径与 MVP 边界见实物资产域词条（ADR-0064）。
- **应用更新（单列小域）→ 壳层机制，与备份域相邻**：自动更新不持账本语义，后端与前端面归壳层（不立业务域 crate 与壳内域目录，ADR-0111 归位判据；决策集合见 ADR-0124）；安装前经备份域更新前备份，自动检查开关与提醒记忆是设备偏好（见参考设置域轻量设置项），提醒弹层消费界面域弹层编排；与多端同步域无交叉——更新分发应用本体，不分发账本数据。
