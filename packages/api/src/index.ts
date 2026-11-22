import { invoke as tauriInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { trackBusy } from "./globalBusy";
import type {
  Account,
  AccountBalance,
  AccountBalanceAdjustInput,
  AccountInput,
  AccountUpdateInput,
  AddFundResult,
  AddInstrumentChannel,
  AddStockInstrumentResult,
  AppBuildInfo,
  BalanceCacheAudit,
  BackupFileInfo,
  AutoBackupState,
  BackupMetaSummary,
  BackupResult,
  Book,
  BookListInfo,
  Budget,
  BudgetAnnualSummary,
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
  CurrencyCumulativePnl,
  CumulativePnlNativeTotal,
  CrossBookInvestmentSummary,
  DashboardOverview,
  DataLocationChangeOutcome,
  DataLocationInfo,
  EncryptionStatus,
  UnlockOutcome,
  RememberPassphraseSupport,
  BootStatus,
  ExchangeRate,
  ExchangeRateInput,
  ExchangeRateSyncReport,
  ExecuteOccurrenceInput,
  FinancialFreedomOverview,
  Holding,
  InstrumentInput,
  InstrumentListFilter,
  InstrumentListResult,
  InvestmentOverview,
  InvestmentTransactionListFilter,
  InvestmentTransactionListResult,
  ItemDailyCost,
  ItemDisposeInput,
  ItemInput,
  ItemWithDailyCost,
  MarketPrice,
  MarketPriceInput,
  ManualPriceInput,
  ManualPriceResult,
  PriceStaleness,
  Policy,
  PolicyInput,
  PolicyStats,
  Merchant,
  MerchantSharesReport,
  MerchantInput,
  MerchantTransactionCount,
  MerchantUpdateInput,
  Insurer,
  InsurerInput,
  InsurerUpdateInput,
  MonthlySummary,
  MoneyWeightedReturnSummary,
  MwrRange,
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
  SavingsGoalInput,
  SavingsGoalProgress,
  SavingsGoalUpdateInput,
  RestoreResult,
  TransactionConvert,
  TransactionSplit,
  TransactionTrade,
  ScheduledTransactionDetail,
  ScheduledTransactionWithExt,
  SubscriptionSpendOverview,
  SyncInstrumentInfoResult,
  SyncRoundReport,
  SyncStatus,
  SyncChannelConfig,
  SyncChannelConfigInput,
  SyncCheckpointInfo,
  SyncCheckpointPublished,
  SyncBootstrapOutcome,
  ParkedOpInfo,
  TrendRange,
  TransactionInput,
  UpdateTransactionInput,
  TransactionListFilter,
  TransactionListResult,
  TransactionOrderSummary,
  TransactionSearchFilter,
  TransactionSearchResult,
  UpdateStatusInput,
  UpdateSubscriptionInput,
  LogLevelState,
  BaseCurrencyState,
  UpdateCheckOutcome,
  UpdateDownloadOutcome,
  UpdateSurface,
} from "@ledger/types";

export { busyVisible } from "./globalBusy";

const invoke = <T>(cmd: string, args?: InvokeArgs): Promise<T> =>
  args === undefined ? trackBusy(tauriInvoke<T>(cmd)) : trackBusy(tauriInvoke<T>(cmd, args));

const periodPlaceholderYear = (period: ReportPeriodRange): number =>
  Number(period.from.slice(0, 4));

export const api = {
  listCurrencies: () => invoke<Currency[]>("list_currencies"),

  listAccounts: () => invoke<Account[]>("list_accounts"),
  createAccount: (input: AccountInput) => invoke<string>("create_account", { input }),
  updateAccount: (id: string, input: AccountUpdateInput) =>
    invoke<void>("update_account", { id, input }),
  adjustAccountBalance: (id: string, input: AccountBalanceAdjustInput) =>
    invoke<string>("adjust_account_balance", { id, input }),
  deleteAccount: (id: string) => invoke<void>("delete_account", { id }),
  archiveAccount: (id: string) => invoke<void>("archive_account", { id }),
  unarchiveAccount: (id: string) => invoke<void>("unarchive_account", { id }),
  listAccountBalances: () => invoke<AccountBalance[]>("list_account_balances"),
  auditBalanceCache: () => invoke<BalanceCacheAudit>("audit_balance_cache"),

  listCategories: (opts?: { includeDeleted?: boolean }) =>
    invoke<Category[]>("list_categories", { includeDeleted: opts?.includeDeleted ?? false }),
  createCategory: (input: CategoryInput) => invoke<string>("create_category", { input }),
  updateCategory: (id: string, input: CategoryUpdateInput) =>
    invoke<void>("update_category", { id, input }),
  reorderCategories: (items: ReorderItem[]) => invoke<void>("reorder_categories", { items }),
  deleteCategory: (id: string) => invoke<void>("delete_category", { id }),

  listMerchants: (opts?: { includeDeleted?: boolean }) =>
    invoke<Merchant[]>("list_merchants", { includeDeleted: opts?.includeDeleted ?? false }),
  createMerchant: (input: MerchantInput) => invoke<string>("create_merchant", { input }),
  updateMerchant: (id: string, input: MerchantUpdateInput) =>
    invoke<void>("update_merchant", { id, input }),
  deleteMerchant: (id: string) => invoke<void>("delete_merchant", { id }),
  listMerchantTransactionCounts: () =>
    invoke<MerchantTransactionCount[]>("list_merchant_transaction_counts"),

  listInsurers: (opts?: { includeDeleted?: boolean }) =>
    invoke<Insurer[]>("list_insurers", { includeDeleted: opts?.includeDeleted ?? false }),
  createInsurer: (input: InsurerInput) => invoke<string>("create_insurer", { input }),
  updateInsurer: (id: string, input: InsurerUpdateInput) =>
    invoke<void>("update_insurer", { id, input }),
  deleteInsurer: (id: string) => invoke<void>("delete_insurer", { id }),

  listTransactions: (filter?: TransactionListFilter | null) =>
    invoke<TransactionListResult>("list_transactions", { filter: filter ?? null }),
  createTransaction: (input: TransactionInput) => invoke<string>("create_transaction", { input }),
  createTransactions: (inputs: TransactionInput[]) =>
    invoke<CreateTransactionResult[]>("create_transactions", { inputs }),
  updateTransaction: (id: string, input: UpdateTransactionInput) =>
    invoke<void>("update_transaction", { id, input }),
  deleteTransaction: (id: string) => invoke<void>("delete_transaction", { id }),

  searchTransactions: (
    query: string,
    page = 1,
    pageSize = 20,
    filter?: TransactionSearchFilter | null,
  ) =>
    invoke<TransactionSearchResult>("search_transactions", {
      query,
      page,
      pageSize,
      amountMinCents: filter?.amountMinCents ?? null,
      amountMaxCents: filter?.amountMaxCents ?? null,
      dateFrom: filter?.dateFrom ?? null,
      dateTo: filter?.dateTo ?? null,
    }),

  listBudgets: () => invoke<Budget[]>("list_budgets"),
  createBudget: (input: BudgetInput) => invoke<string>("create_budget", { input }),
  updateBudget: (id: string, input: BudgetUpdateInput) =>
    invoke<void>("update_budget", { id, input }),
  deleteBudget: (id: string) => invoke<void>("delete_budget", { id }),

  dashboardOverview: () => invoke<DashboardOverview>("dashboard_overview"),
  financialFreedom: () => invoke<FinancialFreedomOverview>("financial_freedom"),

  reportDateRange: () => invoke<ReportDateRange>("report_date_range"),
  monthlySummary: (period: ReportPeriodRange | number) =>
    typeof period === "number"
      ? invoke<MonthlySummary[]>("monthly_summary", { year: period, from: null, to: null })
      : invoke<MonthlySummary[]>("monthly_summary", {
          year: periodPlaceholderYear(period),
          from: period.from,
          to: period.to,
        }),
  merchantShares: (period: ReportPeriodRange, topN: number | null) =>
    invoke<MerchantSharesReport>("merchant_shares", {
      year: periodPlaceholderYear(period),
      from: period.from,
      to: period.to,
      topN,
    }),
  categoryShares: (kind: string, period: ReportPeriodRange) =>
    invoke<CategoryShare[]>("category_shares", {
      kind,
      month: null,
      year: null,
      from: period.from,
      to: period.to,
    }),
  budgetProgress: () => invoke<BudgetProgress[]>("budget_progress"),
  budgetAnnualSummary: () => invoke<BudgetAnnualSummary>("budget_annual_summary"),

  listInstruments: (filter?: InstrumentListFilter | null) =>
    invoke<InstrumentListResult>("list_instruments", { filter: filter ?? null }),
  getTransactionTrade: (id: string) => invoke<TransactionTrade>("get_transaction_trade", { id }),
  getTransactionConvert: (id: string) =>
    invoke<TransactionConvert>("get_transaction_convert", { id }),
  getTransactionSplit: (id: string) => invoke<TransactionSplit>("get_transaction_split", { id }),
  getTransactionOrderSummary: (sourceOrderNo: string) =>
    invoke<TransactionOrderSummary>("get_transaction_order_summary", { sourceOrderNo }),
  createInstrument: (input: InstrumentInput) => invoke<string>("create_instrument", { input }),
  deleteInstrument: (id: string) => invoke<void>("delete_instrument", { id }),
  addFundByCode: (code: string) => invoke<AddFundResult>("add_fund_by_code", { code }),
  addInstrumentByCode: (market: AddInstrumentChannel, code: string) =>
    invoke<AddStockInstrumentResult>("add_instrument_by_code", { market, code }),
  recordManualPrice: (input: ManualPriceInput) =>
    invoke<ManualPriceResult>("record_manual_price", { input }),

  listHoldings: () => invoke<Holding[]>("list_holdings"),

  investmentOverview: () => invoke<InvestmentOverview>("investment_overview"),

  listInvestmentTransactions: (filter?: InvestmentTransactionListFilter | null) =>
    invoke<InvestmentTransactionListResult>("list_investment_transactions", {
      filter: filter ?? null,
    }),

  instrumentPriceStaleness: () => invoke<PriceStaleness>("instrument_price_staleness"),

  portfolioValueTrend: (filter?: TrendRange | null) =>
    invoke<PortfolioValueTrend>("portfolio_value_trend", { filter: filter ?? null }),

  realizedPnlSummary: (filter?: PnlFilter) =>
    invoke<RealizedPnlSummary>("realized_pnl_summary", { filter: filter ?? null }),
  cumulativePnlSummary: () => invoke<CurrencyCumulativePnl[]>("cumulative_pnl_summary"),
  cumulativePnlNativeTotal: () => invoke<CumulativePnlNativeTotal>("cumulative_pnl_native_total"),
  moneyWeightedReturnSummary: (range?: MwrRange | null) =>
    invoke<MoneyWeightedReturnSummary>("money_weighted_return_summary", {
      range: range ?? null,
    }),

  listItems: () => invoke<ItemWithDailyCost[]>("list_items"),
  calculateItemCost: (id: string, referenceDate?: string | null) =>
    invoke<ItemDailyCost>("calculate_item_cost", { id, referenceDate: referenceDate ?? null }),
  createItem: (input: ItemInput) => invoke<string>("create_item", { input }),
  updateItem: (id: string, input: ItemInput) => invoke<void>("update_item", { id, input }),
  disposeItem: (id: string, input: ItemDisposeInput) => invoke<void>("dispose_item", { id, input }),
  deleteItem: (id: string) => invoke<void>("delete_item", { id }),

  listPolicies: () => invoke<Policy[]>("list_policies"),
  listPolicyStats: () => invoke<PolicyStats[]>("list_policy_stats"),
  createPolicy: (input: PolicyInput) => invoke<string>("create_policy", { input }),
  updatePolicy: (id: string, input: PolicyInput) => invoke<void>("update_policy", { id, input }),
  deletePolicy: (id: string) => invoke<void>("delete_policy", { id }),

  listPhysicalAssets: (status?: string) =>
    invoke<PhysicalAssetList>("list_physical_assets", { status: status ?? null }),
  getPhysicalAsset: (id: string) => invoke<PhysicalAsset>("get_physical_asset", { id }),
  createPhysicalAsset: (input: PhysicalAssetInput) =>
    invoke<string>("create_physical_asset", { input }),
  updatePhysicalAsset: (id: string, input: PhysicalAssetUpdateInput) =>
    invoke<void>("update_physical_asset", { id, input }),
  updatePhysicalAssetValuation: (id: string, input: PhysicalAssetValuationInput) =>
    invoke<void>("update_physical_asset_valuation", { id, input }),
  disposePhysicalAsset: (id: string, input: PhysicalAssetDisposeInput) =>
    invoke<void>("dispose_physical_asset", { id, input }),
  deletePhysicalAsset: (id: string) => invoke<void>("delete_physical_asset", { id }),

  savingsGoalProgress: () => invoke<SavingsGoalProgress[]>("savings_goal_progress"),
  createSavingsGoal: (input: SavingsGoalInput) => invoke<string>("create_savings_goal", { input }),
  updateSavingsGoal: (id: string, input: SavingsGoalUpdateInput) =>
    invoke<void>("update_savings_goal", { id, input }),
  archiveSavingsGoal: (id: string) => invoke<void>("archive_savings_goal", { id }),
  unarchiveSavingsGoal: (id: string) => invoke<void>("unarchive_savings_goal", { id }),
  deleteSavingsGoal: (id: string) => invoke<void>("delete_savings_goal", { id }),

  listExchangeRates: () => invoke<ExchangeRate[]>("list_exchange_rates"),
  createExchangeRate: (input: ExchangeRateInput) =>
    invoke<string>("create_exchange_rate", { input }),
  syncExchangeRates: () => invoke<ExchangeRateSyncReport>("sync_exchange_rates"),

  listMarketPrices: () => invoke<MarketPrice[]>("list_market_prices"),
  createMarketPrice: (input: MarketPriceInput) => invoke<string>("create_market_price", { input }),

  createScheduledTransaction: (input: CreateScheduledInput) =>
    invoke<string>("create_scheduled_transaction", { input }),
  listScheduledTransactions: () =>
    invoke<ScheduledTransactionWithExt[]>("list_scheduled_transactions"),
  getScheduledTransactionDetail: (id: string) =>
    invoke<ScheduledTransactionDetail>("get_scheduled_transaction_detail", { id }),
  updateScheduledTransactionStatus: (input: UpdateStatusInput) =>
    invoke<void>("update_scheduled_transaction_status", { input }),
  updateScheduledSubscription: (input: UpdateSubscriptionInput) =>
    invoke<void>("update_scheduled_subscription", { input }),
  executeScheduledOccurrence: (input: ExecuteOccurrenceInput) =>
    invoke<string>("execute_scheduled_occurrence", { input }),
  expandScheduledOccurrences: (id: string) =>
    invoke<string[]>("expand_scheduled_occurrences", { id }),
  subscriptionSpendOverview: () => invoke<SubscriptionSpendOverview>("subscription_spend_overview"),
  setAutoExecutionEnabled: (enabled: boolean) =>
    invoke<void>("set_auto_execution_enabled", { enabled }),

  getAppBuildInfo: () => invoke<AppBuildInfo>("app_build_info"),

  checkForUpdates: () => invoke<UpdateCheckOutcome>("check_for_updates"),
  setAutoUpdateEnabled: (enabled: boolean) => invoke<void>("set_auto_update_enabled", { enabled }),
  getUpdateSurface: () => invoke<UpdateSurface>("get_update_surface"),
  downloadUpdate: () => invoke<UpdateDownloadOutcome>("download_update"),
  installUpdate: () => invoke<void>("install_update"),
  syncInstrumentInfo: () => invoke<SyncInstrumentInfoResult>("sync_instrument_info"),

  createBackup: (targetPath: string) => invoke<BackupResult>("create_backup", { targetPath }),
  restoreBackup: (backupPath: string, passphrase?: string | null) =>
    invoke<RestoreResult>("restore_backup", { backupPath, passphrase: passphrase ?? null }),
  getBackupMeta: (path: string) => invoke<BackupMetaSummary>("get_backup_meta", { path }),
  restartApp: () => invoke<void>("restart_app"),
  listBackups: (dir: string) => invoke<BackupFileInfo[]>("list_backups", { dir }),
  getBookFingerprint: () => invoke<string | null>("get_book_fingerprint"),
  getDeviceId: () => invoke<string | null>("get_device_id"),
  pruneBackups: (dir: string, keep: number) => invoke<PruneResult>("prune_backups", { dir, keep }),
  setAutoBackupDir: (dir: string) => invoke<void>("set_auto_backup_dir", { dir }),
  getAutoBackupState: () => invoke<AutoBackupState>("get_auto_backup_state"),
  setAutoBackupEnabled: (enabled: boolean) => invoke<void>("set_auto_backup_enabled", { enabled }),

  getDataLocationInfo: () => invoke<DataLocationInfo>("get_data_location_info"),
  submitDataLocationChange: (targetDir: string, adoptExisting: boolean) =>
    invoke<DataLocationChangeOutcome>("submit_data_location_change", {
      targetDir,
      adoptExisting,
    }),
  restoreDefaultDataLocation: (adoptExisting: boolean) =>
    invoke<DataLocationChangeOutcome>("restore_default_data_location", { adoptExisting }),

  crossBookInvestmentSummary: () =>
    invoke<CrossBookInvestmentSummary>("cross_book_investment_summary"),

  listBooks: () => invoke<BookListInfo>("list_books"),
  createBook: (name: string) => invoke<Book>("create_book", { name }),
  switchBook: (id: string) => invoke<Book>("switch_book", { id }),
  renameBook: (id: string, name: string) => invoke<Book>("rename_book", { id, name }),
  removeBook: (id: string) => invoke<void>("remove_book", { id }),

  getBootStatus: () => invoke<BootStatus>("get_boot_status"),
  resetAfterStartupFailure: () => invoke<void>("reset_after_startup_failure"),

  getEncryptionStatus: () => invoke<EncryptionStatus>("get_encryption_status"),
  unlockEncryption: (passphrase: string) =>
    invoke<UnlockOutcome>("unlock_encryption", { passphrase }),
  enableEncryption: (passphrase: string) => invoke<void>("enable_encryption", { passphrase }),
  disableEncryption: (passphrase: string) => invoke<void>("disable_encryption", { passphrase }),
  changeEncryptionPassphrase: (passphrase: string, newPassphrase: string) =>
    invoke<void>("change_encryption_passphrase", { passphrase, newPassphrase }),
  resetAfterForgottenPassphrase: () => invoke<void>("reset_after_forgotten_passphrase"),

  getRememberPassphraseSupport: () =>
    invoke<RememberPassphraseSupport>("get_remember_passphrase_support"),
  setRememberPassphrase: (passphrase: string) =>
    invoke<void>("set_remember_passphrase", { passphrase }),
  clearRememberPassphrase: () => invoke<void>("clear_remember_passphrase"),
  unlockWithRememberedPassphrase: () => invoke<UnlockOutcome>("unlock_with_remembered_passphrase"),

  getAiPrompt: () => invoke<string>("get_ai_prompt"),

  openLogDir: () => invoke<void>("open_log_dir"),
  logFrontendError: (message: string) => invoke<void>("log_frontend_error", { message }),

  revealInFileManager: (path: string) => invoke<void>("reveal_in_file_manager", { path }),

  getLogLevel: () => invoke<LogLevelState>("get_log_level"),
  setLogLevel: (level: string) => invoke<void>("set_log_level", { level }),

  getBaseCurrency: () => invoke<BaseCurrencyState>("get_base_currency"),
  setBaseCurrency: (code: string) => invoke<BaseCurrencyState>("set_base_currency", { code }),

  getSyncStatus: () => invoke<SyncStatus>("get_sync_status"),
  syncNow: (passphrase?: string) =>
    invoke<SyncRoundReport>("sync_now", { passphrase: passphrase || null }),
  getParkedOps: () => invoke<ParkedOpInfo[]>("get_parked_ops"),
  getSyncChannelConfig: () => invoke<SyncChannelConfig>("get_sync_channel_config"),
  setSyncChannelConfig: (config: SyncChannelConfigInput) =>
    invoke<void>("set_sync_channel_config", { config }),
  testSyncChannelConnection: (config: SyncChannelConfigInput) =>
    invoke<void>("test_sync_channel_connection", { config }),
  getSyncChannelCheckpoint: () => invoke<SyncCheckpointInfo | null>("get_sync_channel_checkpoint"),
  publishSyncCheckpoint: (passphrase?: string) =>
    invoke<SyncCheckpointPublished>("publish_sync_checkpoint", { passphrase: passphrase || null }),
  bootstrapSyncFromChannel: (passphrase?: string) =>
    invoke<SyncBootstrapOutcome>("bootstrap_sync_from_channel", { passphrase: passphrase || null }),
};
