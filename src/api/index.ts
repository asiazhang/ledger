import { invoke as tauriInvoke, type InvokeArgs } from '@tauri-apps/api/core'
import { trackBusy } from '@/composables/globalBusy'
import type {
  Account,
  AccountBalance,
  AccountBalanceAdjustInput,
  AccountInput,
  AccountUpdateInput,
  AddFundResult,
  BalanceCacheAudit,
  BackupFileInfo,
  AutoBackupState,
  BackupMetaSummary,
  BackupResult,
  Budget,
  BudgetInput,
  BudgetProgress,
  BudgetUpdateInput,
  Category,
  CategoryInput,
  CategoryUpdateInput,
  CategoryShare,
  ReorderItem,
  CreateScheduledInput,
  CreateTransactionResult,
  Currency,
  DashboardOverview,
  DataLocationChangeOutcome,
  DataLocationInfo,
  EncryptionStatus,
  UnlockOutcome,
  RememberPassphraseSupport,
  BootStatus,
  ExchangeRate,
  ExchangeRateInput,
  ExecuteOccurrenceInput,
  FinancialFreedomOverview,
  Holding,
  InstrumentInput,
  InstrumentListFilter,
  InstrumentListResult,
  InstrumentPriceTrend,
  ItemDailyCost,
  ItemDailyTotal,
  ItemDisposeInput,
  ItemInput,
  ItemWithDailyCost,
  MarketPrice,
  MarketPriceInput,
  ManualPriceInput,
  ManualPriceResult,
  Policy,
  PolicyInput,
  PolicyStats,
  Merchant,
  MerchantSharesReport,
  MerchantInput,
  MerchantTransactionCount,
  MerchantUpdateInput,
  MonthlySummary,
  PhysicalAsset,
  PhysicalAssetDisposeInput,
  PhysicalAssetInput,
  PhysicalAssetList,
  PhysicalAssetUpdateInput,
  PhysicalAssetValuationInput,
  PnlFilter,
  PortfolioValueTrend,
  PruneResult,
  ReportDateRange,
  ReportPeriodRange,
  RealizedPnlSummary,
  RestoreResult,
  TransactionTrade,
  ScheduledTransactionDetail,
  ScheduledTransactionWithExt,
  SubscriptionSpendOverview,
  CancelSyncResult,
  SyncHoldingPricesResult,
  TrendRange,
  TransactionInput,
  UpdateTransactionInput,
  TransactionListFilter,
  TransactionListResult,
  TransactionSearchFilter,
  TransactionSearchResult,
  NotePinyinRepairReport,
  UpdateStatusInput,
  UpdateSubscriptionInput,
  LogLevelState,
} from '@/types'

/** 统一 invoke 封装（全局忙碌条收口点，issue #500）：所有 IPC IO 的生命周期自动
 *  纳入忙碌聚合计数——300ms 阈值内的快操作从不点亮，慢 IO 聚合为一条忙碌条；
 *  错误契约不变，原样上抛调用方（成败语义归 Loadable，见 globalBusy 模块头注）。
 *  无参命令保持单参调用形态（不透传 undefined，保住调用点断言与既有契约）。 */
const invoke = <T>(cmd: string, args?: InvokeArgs): Promise<T> =>
  args === undefined
    ? trackBusy(tauriInvoke<T>(cmd))
    : trackBusy(tauriInvoke<T>(cmd, args))

/** 期间起始年（issue #411）：月度汇总/商户排行的遗留 `year` 是已发布命令的必传参数，
 *  期间口径下后端不消费，传期间起始年占位——推导收口一处，不在调用点重复。 */
const periodPlaceholderYear = (period: ReportPeriodRange): number =>
  Number(period.from.slice(0, 4))

export const api = {
  // 币种
  listCurrencies: () => invoke<Currency[]>('list_currencies'),

  // 账户
  listAccounts: () => invoke<Account[]>('list_accounts'),
  createAccount: (input: AccountInput) => invoke<string>('create_account', { input }),
  updateAccount: (id: string, input: AccountUpdateInput) =>
    invoke<void>('update_account', { id, input }),
  adjustAccountBalance: (id: string, input: AccountBalanceAdjustInput) =>
    invoke<string>('adjust_account_balance', { id, input }),
  deleteAccount: (id: string) => invoke<void>('delete_account', { id }),
  listAccountBalances: () => invoke<AccountBalance[]>('list_account_balances'),
  /** 余额缓存手动审计（issue #491）：全账户实时重算 vs 缓存，修复差异并返回差异报告 */
  auditBalanceCache: () => invoke<BalanceCacheAudit>('audit_balance_cache'),

  // 分类
  // includeDeleted=true 返回含软删全量（URL 下钻校验映射需含软删但仍有历史交易的分类，issue #377）
  listCategories: (opts?: { includeDeleted?: boolean }) =>
    invoke<Category[]>('list_categories', { includeDeleted: opts?.includeDeleted ?? false }),
  createCategory: (input: CategoryInput) => invoke<string>('create_category', { input }),
  updateCategory: (id: string, input: CategoryUpdateInput) =>
    invoke<void>('update_category', { id, input }),
  reorderCategories: (items: ReorderItem[]) =>
    invoke<void>('reorder_categories', { items }),
  deleteCategory: (id: string) => invoke<void>('delete_category', { id }),

  // 商户（issue #188 / ADR-0028）：参考数据字典命令面，随 ledger:changed 失效信号重拉
  // includeDeleted=true 返回含软删全量（交易筛选下拉需含软删但仍有历史交易的商户，issue #191）
  listMerchants: (opts?: { includeDeleted?: boolean }) =>
    invoke<Merchant[]>('list_merchants', { includeDeleted: opts?.includeDeleted ?? false }),
  createMerchant: (input: MerchantInput) => invoke<string>('create_merchant', { input }),
  updateMerchant: (id: string, input: MerchantUpdateInput) =>
    invoke<void>('update_merchant', { id, input }),
  deleteMerchant: (id: string) => invoke<void>('delete_merchant', { id }),
  /** 商户关联交易计数（issue #445，毛笔数口径）：含软删商户、无引用计 0，实时推导不落库 */
  listMerchantTransactionCounts: () =>
    invoke<MerchantTransactionCount[]>('list_merchant_transaction_counts'),

  // 交易
  listTransactions: (filter?: TransactionListFilter | null) =>
    invoke<TransactionListResult>('list_transactions', { filter: filter ?? null }),
  createTransaction: (input: TransactionInput) =>
    invoke<string>('create_transaction', { input }),
  createTransactions: (inputs: TransactionInput[]) =>
    invoke<CreateTransactionResult[]>('create_transactions', { inputs }),
  /** 全字段替换既有交易（issue #178）：与 HTTP PUT /api/v1/transactions/{id} 同一行为层权威；
   * 幂等键/内容哈希不可编辑（入参类型不含该字段）。不存在/已删除报 NotFound。 */
  updateTransaction: (id: string, input: UpdateTransactionInput) =>
    invoke<void>('update_transaction', { id, input }),
  deleteTransaction: (id: string) => invoke<void>('delete_transaction', { id }),

  // 交易搜索
  searchTransactions: (
    query: string,
    page = 1,
    pageSize = 20,
    filter?: TransactionSearchFilter | null,
  ) =>
    invoke<TransactionSearchResult>('search_transactions', {
      query,
      page,
      pageSize,
      amountMinCents: filter?.amountMinCents ?? null,
      amountMaxCents: filter?.amountMaxCents ?? null,
      dateFrom: filter?.dateFrom ?? null,
      dateTo: filter?.dateTo ?? null,
    }),
  /** 备注拼音一键修复（issue #513）：显式回填全部积压并返回报告（回填行数 / 是否收敛 / 失败原因），幂等 */
  repairNotePinyin: () => invoke<NotePinyinRepairReport>('repair_note_pinyin'),

  // 预算
  listBudgets: () => invoke<Budget[]>('list_budgets'),
  createBudget: (input: BudgetInput) => invoke<string>('create_budget', { input }),
  updateBudget: (id: string, input: BudgetUpdateInput) =>
    invoke<void>('update_budget', { id, input }),
  deleteBudget: (id: string) => invoke<void>('delete_budget', { id }),

  // 首页财务全貌
  dashboardOverview: () => invoke<DashboardOverview>('dashboard_overview'),
  // 财务自由度（issue #343）：可投资资产 × 3% ÷ 年度预算总额，只读实时聚合
  financialFreedom: () => invoke<FinancialFreedomOverview>('financial_freedom'),

  // 报表
  // 日期筛选范围（issue #266 / #389）：数据驱动极值日期对，QuickTimeRange 钳制输入
  reportDateRange: () => invoke<ReportDateRange>('report_date_range'),
  // 月度汇总（issue #411 期间化）：报表页传 { from, to }（YYYY-MM-DD 含边界）走期间口径；
  // 概览页年度趋势仍传年份数字走遗留口径（遗留 year 参数已冻结保留，待概览页另票接入）
  monthlySummary: (period: ReportPeriodRange | number) =>
    typeof period === 'number'
      ? invoke<MonthlySummary[]>('monthly_summary', { year: period, from: null, to: null })
      : invoke<MonthlySummary[]>('monthly_summary', {
          year: periodPlaceholderYear(period),
          from: period.from,
          to: period.to,
        }),
  // 商户消费排行（issue #411 期间化；#588 可选 top_n 参数只增 + 载荷改为
  // { rows, total_cents }——内部 IPC 契约，前端唯一调用方同发更新）：
  // 报表页按期间查询（遗留 year 冻结不再使用）；topN null = 全量（既有行为不变）。
  // 键名必须 camelCase（topN）：Tauri IPC 按 Rust 参数名的 lowerCamelCase 绑定，
  // snake_case 键静默失配为 None（topN 失效回全量的回归即源于此）。
  merchantShares: (period: ReportPeriodRange, topN: number | null) =>
    invoke<MerchantSharesReport>('merchant_shares', {
      year: periodPlaceholderYear(period),
      from: period.from,
      to: period.to,
      topN,
    }),
  // 分类份额（issue #411 期间化）：报表页按期间查询（遗留 month/year 冻结不再使用）
  categoryShares: (kind: string, period: ReportPeriodRange) =>
    invoke<CategoryShare[]>('category_shares', {
      kind,
      month: null,
      year: null,
      from: period.from,
      to: period.to,
    }),
  budgetProgress: () => invoke<BudgetProgress[]>('budget_progress'),

  // 金融工具
  listInstruments: (filter?: InstrumentListFilter | null) =>
    invoke<InstrumentListResult>('list_instruments', { filter: filter ?? null }),
  // 交易买卖明细（issue #180）：buy/sell 交易编辑回填数据源（扩展表投影，非买卖交易 NotFound）
  getTransactionTrade: (id: string) =>
    invoke<TransactionTrade>('get_transaction_trade', { id }),
  createInstrument: (input: InstrumentInput) =>
    invoke<string>('create_instrument', { input }),
  // 自建标的删除（issue #292 / ADR-0036）：仅手动来源且无买卖流水引用可删，
  // 守卫在后端前置检查，同步来源拒删
  deleteInstrument: (id: string) => invoke<void>('delete_instrument', { id }),
  // 按代码即拉添加场外基金（issue #301 / ADR-0038）：东财回填名称/分类/最新净值
  addFundByCode: (code: string) => invoke<AddFundResult>('add_fund_by_code', { code }),
  // 手动报价（issue #291 / ADR-0036）：无行情来源标的的「日期 + 价格」单点录入，
  // 一条通道两个落点（现价缓存 upsert + 价格历史周采样幂等覆盖）；实际写入
  // 任一落点后端广播价格失效信号，调用方依赖信号消费方刷新，零手动重拉
  recordManualPrice: (input: ManualPriceInput) =>
    invoke<ManualPriceResult>('record_manual_price', { input }),

  // 持仓
  listHoldings: () => invoke<Holding[]>('list_holdings'),

  // 走势（issue #138）：单标的周采样价格序列与组合市值周点曲线
  instrumentPriceTrend: (instrumentId: string, filter?: TrendRange | null) =>
    invoke<InstrumentPriceTrend>('instrument_price_trend', {
      instrumentId,
      filter: filter ?? null,
    }),
  portfolioValueTrend: (filter?: TrendRange | null) =>
    invoke<PortfolioValueTrend>('portfolio_value_trend', { filter: filter ?? null }),

  // 已实现盈亏汇总
  realizedPnlSummary: (filter?: PnlFilter) =>
    invoke<RealizedPnlSummary>('realized_pnl_summary', { filter: filter ?? null }),

  // 物品（issue #116）：独立领域（非参考数据），写入后由后端发 ledger:changed
  listItems: () => invoke<ItemWithDailyCost[]>('list_items'),
  // 全部在用物品每天成本合计（issue #122）：后端聚合（默认币种），dashboard 汇总卡消费
  itemDailyTotal: () => invoke<ItemDailyTotal>('item_daily_total'),
  // 自选参考日重算（issue #121）：referenceDate 省略/null → 缺省目标日（在用今天/已处置处置日）
  calculateItemCost: (id: string, referenceDate?: string | null) =>
    invoke<ItemDailyCost>('calculate_item_cost', { id, referenceDate: referenceDate ?? null }),
  createItem: (input: ItemInput) => invoke<string>('create_item', { input }),
  updateItem: (id: string, input: ItemInput) => invoke<void>('update_item', { id, input }),
  disposeItem: (id: string, input: ItemDisposeInput) =>
    invoke<void>('dispose_item', { id, input }),
  deleteItem: (id: string) => invoke<void>('delete_item', { id }),

  // 保单（issue #360 / ADR-0051）：独立领域（静态档案），写入后由后端发 ledger:changed
  listPolicies: () => invoke<Policy[]>('list_policies'),
  // 保单视角统计（issue #363）：只读聚合，实时推导不落库
  listPolicyStats: () => invoke<PolicyStats[]>('list_policy_stats'),
  createPolicy: (input: PolicyInput) => invoke<string>('create_policy', { input }),
  updatePolicy: (id: string, input: PolicyInput) => invoke<void>('update_policy', { id, input }),
  deletePolicy: (id: string) => invoke<void>('delete_policy', { id }),

  // 实物资产（issue #466 / ADR-0064）：独立领域（估值档案），写入后由后端发 ledger:changed
  listPhysicalAssets: (status?: string) =>
    invoke<PhysicalAssetList>('list_physical_assets', { status: status ?? null }),
  getPhysicalAsset: (id: string) => invoke<PhysicalAsset>('get_physical_asset', { id }),
  createPhysicalAsset: (input: PhysicalAssetInput) =>
    invoke<string>('create_physical_asset', { input }),
  // 编辑档案（issue #467 T2）：仅名称 / 购买信息，估值不经本入口变更
  updatePhysicalAsset: (id: string, input: PhysicalAssetUpdateInput) =>
    invoke<void>('update_physical_asset', { id, input }),
  // 更新估值（issue #467 T2）：追加一条估值历史行，当前估值变为最新一条
  updatePhysicalAssetValuation: (id: string, input: PhysicalAssetValuationInput) =>
    invoke<void>('update_physical_asset_valuation', { id, input }),
  // 处置（issue #468 T3）：状态标记转已处置 + 处置信息纯记录，退出默认列表与在持合计
  disposePhysicalAsset: (id: string, input: PhysicalAssetDisposeInput) =>
    invoke<void>('dispose_physical_asset', { id, input }),
  // 软删除（issue #468 T3）：数据与估值历史保留，退出列表与合计
  deletePhysicalAsset: (id: string) => invoke<void>('delete_physical_asset', { id }),

  // 汇率
  listExchangeRates: () => invoke<ExchangeRate[]>('list_exchange_rates'),
  createExchangeRate: (input: ExchangeRateInput) =>
    invoke<string>('create_exchange_rate', { input }),

  // 行情价格
  listMarketPrices: () => invoke<MarketPrice[]>('list_market_prices'),
  createMarketPrice: (input: MarketPriceInput) =>
    invoke<string>('create_market_price', { input }),

  // 定时交易
  createScheduledTransaction: (input: CreateScheduledInput) =>
    invoke<string>('create_scheduled_transaction', { input }),
  listScheduledTransactions: () =>
    invoke<ScheduledTransactionWithExt[]>('list_scheduled_transactions'),
  getScheduledTransactionDetail: (id: string) =>
    invoke<ScheduledTransactionDetail>('get_scheduled_transaction_detail', { id }),
  updateScheduledTransactionStatus: (input: UpdateStatusInput) =>
    invoke<void>('update_scheduled_transaction_status', { input }),
  // 订阅编辑（issue #162，ADR-0023 决策三）：仅非金额字段，携带金额被后端显式拒绝
  updateScheduledSubscription: (input: UpdateSubscriptionInput) =>
    invoke<void>('update_scheduled_subscription', { input }),
  executeScheduledOccurrence: (input: ExecuteOccurrenceInput) =>
    invoke<string>('execute_scheduled_occurrence', { input }),
  expandScheduledOccurrences: (id: string) =>
    invoke<string[]>('expand_scheduled_occurrences', { id }),
  // 订阅实际花费总览（issue #160，实际口径：忠实流水不摊销）
  subscriptionSpendOverview: () =>
    invoke<SubscriptionSpendOverview>('subscription_spend_overview'),
  // 推送设备级「自动执行」开关到后端运行时镜像（issue #307 / ADR-0042）：
  // 真源在本机 localStorage 设备偏好，应用启动与变更时调用
  setAutoExecutionEnabled: (enabled: boolean) =>
    invoke<void>('set_auto_execution_enabled', { enabled }),

  // 数据同步
  syncInstruments: () => invoke<void>('sync_instruments'),
  // 请求中断进行中的全量同步（issue #104）：返回是否确实中断 + 提示文案
  cancelSyncInstruments: () => invoke<CancelSyncResult>('cancel_sync_instruments'),
  // 同步持仓价格（增量同步）：仅刷新当前持仓股票的最新价，返回同步/跳过统计
  syncHoldingPrices: () => invoke<SyncHoldingPricesResult>('sync_holding_prices'),

  // 备份与恢复
  createBackup: (targetPath: string) => invoke<BackupResult>('create_backup', { targetPath }),
  // 密文备份需附带备份所在库的主口令（issue #572 / ADR-0075 决策 7）；明文备份传 null
  restoreBackup: (backupPath: string, passphrase?: string | null) =>
    invoke<RestoreResult>('restore_backup', { backupPath, passphrase: passphrase ?? null }),
  // 读取单个备份文件元数据摘要（来源 + 加密标记，issue #572）：恢复确认弹窗消费
  getBackupMeta: (path: string) => invoke<BackupMetaSummary>('get_backup_meta', { path }),
  restartApp: () => invoke<void>('restart_app'),
  listBackups: (dir: string) => invoke<BackupFileInfo[]>('list_backups', { dir }),
  pruneBackups: (dir: string, keep: number) => invoke<PruneResult>('prune_backups', { dir, keep }),
  // 同步备份目录镜像到后端（自动备份调度用，issue #125）：启动/变更时调用
  setAutoBackupDir: (dir: string) => invoke<void>('set_auto_backup_dir', { dir }),
  // 自动备份设置页状态读写（issue #128）：开关与上次自动备份时间，存 ledger.db
  getAutoBackupState: () => invoke<AutoBackupState>('get_auto_backup_state'),
  setAutoBackupEnabled: (enabled: boolean) =>
    invoke<void>('set_auto_backup_enabled', { enabled }),

  // DataLocation（issue #133）：查询 / 更改意图 / 恢复默认，意图落盘后下次启动生效
  getDataLocationInfo: () => invoke<DataLocationInfo>('get_data_location_info'),
  submitDataLocationChange: (targetDir: string, adoptExisting: boolean) =>
    invoke<DataLocationChangeOutcome>('submit_data_location_change', {
      targetDir,
      adoptExisting,
    }),
  restoreDefaultDataLocation: (adoptExisting: boolean) =>
    invoke<DataLocationChangeOutcome>('restore_default_data_location', { adoptExisting }),

  // 启动状态与启动失败恢复（issue #601 / ADR-0075 决策 5 修订）：前端启动探测
  // 唯一入口（主界面/解锁屏/失败恢复屏三态选择）+ 失败恢复屏的重置通道。
  getBootStatus: () => invoke<BootStatus>('get_boot_status'),
  resetAfterStartupFailure: () => invoke<void>('reset_after_startup_failure'),

  // 加密模式（issue #570/#571 / ADR-0075）：状态查询 / 解锁 / 转换三形态 / 忘记口令重置（#573）
  getEncryptionStatus: () => invoke<EncryptionStatus>('get_encryption_status'),
  unlockEncryption: (passphrase: string) =>
    invoke<UnlockOutcome>('unlock_encryption', { passphrase }),
  enableEncryption: (passphrase: string) => invoke<void>('enable_encryption', { passphrase }),
  disableEncryption: (passphrase: string) => invoke<void>('disable_encryption', { passphrase }),
  changeEncryptionPassphrase: (passphrase: string, newPassphrase: string) =>
    invoke<void>('change_encryption_passphrase', { passphrase, newPassphrase }),
  resetAfterForgottenPassphrase: () => invoke<void>('reset_after_forgotten_passphrase'),

  // 本机记住主口令（issue #574 / ADR-0075 决策 3）：钥匙串缓存 + macOS 生物认证门。
  // 「记住」偏好开关是前端 localStorage 轻量设置项（app store），此处只暴露后端命令。
  getRememberPassphraseSupport: () =>
    invoke<RememberPassphraseSupport>('get_remember_passphrase_support'),
  setRememberPassphrase: (passphrase: string) =>
    invoke<void>('set_remember_passphrase', { passphrase }),
  clearRememberPassphrase: () => invoke<void>('clear_remember_passphrase'),
  unlockWithRememberedPassphrase: () =>
    invoke<UnlockOutcome>('unlock_with_remembered_passphrase'),

  // AI
  getAiPrompt: () => invoke<string>('get_ai_prompt'),

  // 日志（issue #283）：打开日志目录（系统文件管理器展示，按天滚动、保留 7 天）
  openLogDir: () => invoke<void>('open_log_dir'),

  // 日志等级（spec #611，About 页「关于」Tab）：读持久化档位 + 校验闭集写入 +
  // 运行期接管滤镜（文件/终端共用同一滤镜、立即生效、跨启动保留）。
  // 界面展示的是持久化档位；显式 RUST_LOG 环境变量在本次启动内优先且不写库。
  getLogLevel: () => invoke<LogLevelState>('get_log_level'),
  setLogLevel: (level: string) => invoke<void>('set_log_level', { level }),
}
