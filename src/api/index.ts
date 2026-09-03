import { invoke } from '@tauri-apps/api/core'
import type {
  Account,
  AccountBalance,
  AccountBalanceAdjustInput,
  AccountInput,
  AccountUpdateInput,
  AddFundResult,
  BackupFileInfo,
  AutoBackupState,
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
  MerchantShare,
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
  UpdateStatusInput,
  UpdateSubscriptionInput,
} from '@/types'

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
  // 商户消费排行（issue #411 期间化）：报表页按期间查询（遗留 year 冻结不再使用）
  merchantShares: (period: ReportPeriodRange) =>
    invoke<MerchantShare[]>('merchant_shares', {
      year: periodPlaceholderYear(period),
      from: period.from,
      to: period.to,
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
  restoreBackup: (backupPath: string) => invoke<RestoreResult>('restore_backup', { backupPath }),
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

  // AI
  getAiPrompt: () => invoke<string>('get_ai_prompt'),

  // 日志（issue #283）：打开日志目录（系统文件管理器展示，按天滚动、保留 7 天）
  openLogDir: () => invoke<void>('open_log_dir'),
}
