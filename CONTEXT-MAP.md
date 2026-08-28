# CONTEXT-MAP — 领域词汇表地图

Ledger 的领域词汇表按自然域拆分：本文件列出全部分域、各自位置与彼此关系。**动手前先读本地图，再按改动主题选读相关分域词汇表与相关 ADR**（决策型叙述与 MVP 取舍归 `docs/adr/`，词汇表条目只保留定义与 ADR 指针）。与代码行为冲突时，以代码为准并同步修正词汇表。

## 结构约定

- 全部分域文件与地图**同层集中存放**（仓库根目录 `CONTEXT-*.md`），不按源码目录散布——本仓库的域与目录结构不对齐（一个域横跨前后端），按目录放会误导。
- **术语全库唯一**：任一术语只在一份分域文件中定义，各分域文件间不复制定义。
- **跨域共享术语归核心交易域**：被多个域消费的概念（Transaction、Amount Model、Transaction Kind Mapping、Category、DefaultCurrency 等）只在核心交易域定义；其他域以「见核心交易域 X」引用，不复制定义。
- **新增术语进哪份文件**：按自然域归属放入对应分域；若它是被多域消费的共享概念，进核心交易域；单列小域只接纳体量小且与既有域边界清晰的独立概念（如物品域）。
- 一致性校验（地图与文件对应、术语唯一、导航一致）由独立检查脚本守住，挂入 `scripts/check.sh` 质量门槛（见 issue #166）。

## 分域一览

| # | 分域 | 文件 | 条目主题 |
|---|------|------|----------|
| 1 | 核心交易 | [`CONTEXT-core.md`](CONTEXT-core.md) | Transaction、InvolvingAccount、Amount Model、Transaction Kind Mapping、Category、DefaultCurrency、TransactionSearch、万分位分组、耗时日志、慢查询 |
| 2 | 定时计划 | [`CONTEXT-scheduled-plans.md`](CONTEXT-scheduled-plans.md) | ScheduledTransaction 及三种业务形态（分期/订阅/定时转账）、Occurrence、Plan Lifecycle、Timing、Counterparty、Recurrence Rule、Failure Policy |
| 3 | 投资域 | [`CONTEXT-investment.md`](CONTEXT-investment.md) | Instrument、MarketPrice、PriceHistory、FxRateHistory、PortfolioValueTrend、Holding、NetWorth、Investment、InvestedInstrument、InstrumentSync、HoldingPriceSync |
| 4 | AI 导入 | [`CONTEXT-ai-import.md`](CONTEXT-ai-import.md) | AI API、AIReadbackVerification、AICleanupDeletion、AICleanupModify、ImportDedup、IdempotencyKey、BlackHoleAccount、AIPrompt |
| 5 | 参考数据与设置 | [`CONTEXT-reference-settings.md`](CONTEXT-reference-settings.md) | Reference Data、Appearance、AppSettings、轻量设置项、DataLocation |
| 6 | 备份与数据文件 | [`CONTEXT-backup-datafiles.md`](CONTEXT-backup-datafiles.md) | Backup、Restore、RestoreSafetyBackup、BackupDirectory、BackupRetentionLimit、BackupPruning、ManagedBackup、ManualBackup、BackupTrigger、AutoBackup、DirtyMarker |
| 7 | 界面状态与交互 | [`CONTEXT-ui-interaction.md`](CONTEXT-ui-interaction.md) | WindowState、ViewState、ViewShortcut、CreateShortcut、Overlay Suppression、ESC 键语义、原生右键菜单 |
| 8 | 物品 | [`CONTEXT-item.md`](CONTEXT-item.md) | Item、DailyUsageCost |

## 域间关系

关系即归属逻辑——一个术语放哪个域，由「谁定义它、谁消费它」决定：

- **核心交易是被所有域消费的底座**。Transaction 与 Amount Model（raw/native 分离 + kind→度量符号矩阵）定义「一笔资金变动长什么样」，Category / DefaultCurrency 是它依赖的字典与折算基准；定时计划、投资、AI 导入、物品四域产出的最终都是核心域的交易流水（或以它为对账基准），所以这些共享概念只在核心域定义。
- **定时计划 → 核心交易**：ScheduledTransaction 及其三种业务形态（InstallmentPlan / Subscription / ScheduledTransfer）是生成核心域 Transaction 的规则与模板，每期触发落一条流水（Occurrence）；生命周期、周期规则、失败策略等 MVP 决策在 ADR-0024。
- **投资域 → 核心交易**：buy/sell 首先是核心域 Transaction 的 kind，Investment 是它背后的持仓/盈亏载体；市值与净资产折算消费核心域 DefaultCurrency，历史折算经本域 FxRateHistory。
- **AI 导入 → 核心交易 + 参考数据**：AI 经本域 AI API 幂等写入核心域 Transaction 与参考数据（账户/分类/币种），ImportDedup / IdempotencyKey 是写入侧的去重契约，读回验证（AIReadbackVerification）按核心域 InvolvingAccount 与余额口径对账；BlackHoleAccount 是参考数据中的特殊账户，因由导入流程预置与消费而归本域。
- **参考数据与设置 → 被核心交易引用、与备份相邻**：Reference Data（账户/分类/币种字典）被核心域 Transaction 以外键引用；AppSettings 是后端配置与运行时状态的权威落点，备份域的调度状态（AutoBackup / DirtyMarker）存于其中——这是它与备份域的相邻点；DataLocation 因「建连前必须可读」成为库外引导配置的唯一例外。
- **备份与数据文件 ↔ 各域相邻而不交叉**：Backup/Restore 是文件级整库快照通道，与 AI 导入（语义级写入）、行情同步（投资域）互不交叉；备份不迁移界面状态（界面域）与设备偏好（参考设置域 / 核心域 DefaultCurrency）。
- **界面状态与交互 → 只读消费各域**：WindowState / ViewState / 快捷键 / 弹层抑制是纯界面层概念，不持业务数据；搜索（核心域 TransactionSearch）复用交易列表信息但搜索词不持久化（与 ViewState 边界一致）。
- **物品（单列小域）→ 可选挂靠核心交易**：Item 是与参考数据、交易流水、投资标的并列的独立领域概念，自包含总成本、不进字典；可选关联一笔核心域 Transaction 仅用于带出成本与溯源，不建立反向引用。
