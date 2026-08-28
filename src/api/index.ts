import { invoke } from '@tauri-apps/api/core'
import type {
  Account,
  AccountBalance,
  AccountInput,
  BackupFileInfo,
  AutoBackupState,
  BackupResult,
  Budget,
  BudgetInput,
  BudgetProgress,
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
  ItemDisposeInput,
  ItemInput,
  ItemWithDailyCost,
  MarketPrice,
  MarketPriceInput,
  MonthlySummary,
  PnlFilter,
  PortfolioValueTrend,
  PruneResult,
  RealizedPnlSummary,
  RestoreResult,
  ScheduledTransactionDetail,
  ScheduledTransactionWithExt,
  SubscriptionSpendOverview,
  CancelSyncResult,
  SyncHoldingPricesResult,
  TrendRange,
  TransactionInput,
  TransactionListFilter,
  TransactionListResult,
  TransactionSearchFilter,
  TransactionSearchResult,
  UpdateStatusInput,
} from '@/types'

export const api = {
  // 币种
  listCurrencies: () => invoke<Currency[]>('list_currencies'),

  // 账户
  listAccounts: () => invoke<Account[]>('list_accounts'),
  createAccount: (input: AccountInput) => invoke<string>('create_account', { input }),
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

  // 交易
  listTransactions: (filter?: TransactionListFilter | null) =>
    invoke<TransactionListResult>('list_transactions', { filter: filter ?? null }),
  createTransaction: (input: TransactionInput) =>
    invoke<string>('create_transaction', { input }),
  createTransactions: (inputs: TransactionInput[]) =>
    invoke<CreateTransactionResult[]>('create_transactions', { inputs }),
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
  deleteBudget: (id: string) => invoke<void>('delete_budget', { id }),

  // 首页财务全貌
  dashboardOverview: () => invoke<DashboardOverview>('dashboard_overview'),

  // 报表
  monthlySummary: (year: number) => invoke<MonthlySummary[]>('monthly_summary', { year }),
  categoryShares: (kind: string, month?: string) =>
    invoke<CategoryShare[]>('category_shares', { kind, month: month ?? null }),
  budgetProgress: () => invoke<BudgetProgress[]>('budget_progress'),

  // 金融工具
  listInstruments: (filter?: InstrumentListFilter | null) =>
    invoke<InstrumentListResult>('list_instruments', { filter: filter ?? null }),
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
