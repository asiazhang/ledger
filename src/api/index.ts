import { invoke } from '@tauri-apps/api/core'
import type {
  Account,
  AccountBalance,
  AccountBalanceAdjustInput,
  AccountInput,
  AccountUpdateInput,
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
  Merchant,
  MerchantShare,
  MerchantInput,
  MerchantUpdateInput,
  MonthlySummary,
  PnlFilter,
  PortfolioValueTrend,
  PruneResult,
  YearRange,
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
  listCategories: () => invoke<Category[]>('list_categories'),
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

  // 报表
  // 年份筛选范围（issue #266）：数据驱动闭区间，前端拉一次平铺年份下拉
  reportYearRange: () => invoke<YearRange>('report_year_range'),
  monthlySummary: (year: number) => invoke<MonthlySummary[]>('monthly_summary', { year }),
  merchantShares: (year: number) => invoke<MerchantShare[]>('merchant_shares', { year }),
  categoryShares: (kind: string, month?: string) =>
    invoke<CategoryShare[]>('category_shares', { kind, month: month ?? null }),
  budgetProgress: () => invoke<BudgetProgress[]>('budget_progress'),

  // 金融工具
  listInstruments: (filter?: InstrumentListFilter | null) =>
    invoke<InstrumentListResult>('list_instruments', { filter: filter ?? null }),
  // 交易买卖明细（issue #180）：buy/sell 交易编辑回填数据源（扩展表投影，非买卖交易 NotFound）
  getTransactionTrade: (id: string) =>
    invoke<TransactionTrade>('get_transaction_trade', { id }),
  createInstrument: (input: InstrumentInput) =>
    invoke<string>('create_instrument', { input }),

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
}
