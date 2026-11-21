import {
  getTransactionConvert,
  getTransactionSplit,
  getTransactionTrade,
  listInvestmentTransactions,
} from "./investment_txns";
import {
  addFundByCode,
  addInstrumentByCode,
  createInstrument,
  deleteInstrument,
  listInstruments,
} from "./investment";
import {
  budgetAnnualSummary,
  budgetProgress,
  createBudget,
  deleteBudget,
  listBudgets,
  updateBudget,
} from "./budget";
import {
  adjustAccountBalance,
  archiveAccount,
  auditBalanceCache,
  createAccount,
  createCategory,
  createInsurer,
  createMerchant,
  createTransaction,
  deleteAccount,
  deleteCategory,
  deleteInsurer,
  deleteMerchant,
  deleteTransaction,
  getBootStatus,
  listAccountBalances,
  listAccounts,
  listCategories,
  listCurrencies,
  listInsurers,
  listMerchantTransactionCounts,
  listMerchants,
  listTransactions,
  reorderCategories,
  unarchiveAccount,
  updateAccount,
  updateCategory,
  updateInsurer,
  updateMerchant,
  updateTransaction,
} from "./ledger";
import { categoryShares, merchantShares, monthlySummary, reportDateRange } from "./reports";
import { crossBookInvestmentSummary, dashboardOverview, financialFreedom } from "./dashboard";
import { createTransactions, getTransactionOrderSummary, searchTransactions } from "./transactions";
import {
  appBuildInfo,
  getAiPrompt,
  getBaseCurrency,
  getLogLevel,
  logFrontendError,
  openLogDir,
  revealInFileManager,
  restartApp,
  setAutoExecutionEnabled,
  setAutoUpdateEnabled,
  setBaseCurrency,
  setLogLevel,
} from "./settings";
import {
  createPhysicalAsset,
  deletePhysicalAsset,
  disposePhysicalAsset,
  getPhysicalAsset,
  listPhysicalAssets,
  updatePhysicalAsset,
  updatePhysicalAssetValuation,
} from "./physical-asset";
import {
  archiveSavingsGoal,
  createSavingsGoal,
  deleteSavingsGoal,
  savingsGoalProgress,
  unarchiveSavingsGoal,
  updateSavingsGoal,
} from "./savings-goal";
import { reseed } from "./state";
import {
  createScheduledTransaction,
  executeScheduledOccurrence,
  expandScheduledOccurrences,
  getScheduledTransactionDetail,
  listScheduledTransactions,
  subscriptionSpendOverview,
  updateScheduledSubscription,
  updateScheduledTransactionStatus,
} from "./scheduled";
import { registerMockCommand } from "./tauri/core";
import {
  cumulativePnlNativeTotal,
  cumulativePnlSummary,
  instrumentPriceStaleness,
  investmentOverview,
  listHoldings,
  moneyWeightedReturnSummary,
  portfolioValueTrend,
  realizedPnlSummary,
  syncInstrumentInfo,
} from "./investment_aggregates";
import {
  calculateItemCost,
  createItem,
  deleteItem,
  disposeItem,
  listItems,
  updateItem,
} from "./item";
import { createPolicy, deletePolicy, listPolicies, listPolicyStats, updatePolicy } from "./policy";
import {
  changeEncryptionPassphrase,
  clearRememberPassphrase,
  disableEncryption,
  enableEncryption,
  getEncryptionStatus,
  getRememberPassphraseSupport,
  resetAfterForgottenPassphrase,
  resetAfterStartupFailure,
  setRememberPassphrase,
  unlockEncryption,
  unlockWithRememberedPassphrase,
} from "./encryption";
import { createBook, listBooks, removeBook, renameBook, switchBook } from "./books";
import {
  createExchangeRate,
  createMarketPrice,
  listExchangeRates,
  listMarketPrices,
  recordManualPrice,
  syncExchangeRates,
} from "./fx-prices";
import { checkForUpdates, downloadUpdate, getUpdateSurface, installUpdate } from "./app-update";
import {
  bootstrapSyncFromChannel,
  getParkedOps,
  getSyncChannelCheckpoint,
  getSyncChannelConfig,
  getSyncStatus,
  publishSyncCheckpoint,
  setSyncChannelConfig,
  syncNow,
  testSyncChannelConnection,
} from "./sync";
import {
  createBackup,
  getAutoBackupState,
  getBackupMeta,
  getBookFingerprint,
  getDeviceId,
  listBackups,
  pruneBackups,
  restoreBackup,
  setAutoBackupDir,
  setAutoBackupEnabled,
} from "./backup";
import {
  getDataLocationInfo,
  restoreDefaultDataLocation,
  submitDataLocationChange,
} from "./data-location";

export function installMockBackend(): void {
  reseed();

  registerMockCommand("get_boot_status", () => getBootStatus());

  registerMockCommand("set_auto_execution_enabled", (args) => setAutoExecutionEnabled(args));
  registerMockCommand("set_auto_update_enabled", (args) => setAutoUpdateEnabled(args));
  registerMockCommand("get_base_currency", () => getBaseCurrency());
  registerMockCommand("set_base_currency", (args) => setBaseCurrency(args));
  registerMockCommand("get_log_level", () => getLogLevel());
  registerMockCommand("set_log_level", (args) => setLogLevel(args));
  registerMockCommand("app_build_info", () => appBuildInfo());
  registerMockCommand("get_ai_prompt", () => getAiPrompt());
  registerMockCommand("open_log_dir", () => openLogDir());
  registerMockCommand("log_frontend_error", (args) => logFrontendError(args));
  registerMockCommand("reveal_in_file_manager", (args) => revealInFileManager(args));
  registerMockCommand("restart_app", () => restartApp());

  registerMockCommand("get_encryption_status", () => getEncryptionStatus());
  registerMockCommand("unlock_encryption", (args) => unlockEncryption(args));
  registerMockCommand("enable_encryption", (args) => enableEncryption(args));
  registerMockCommand("disable_encryption", (args) => disableEncryption(args));
  registerMockCommand("change_encryption_passphrase", (args) => changeEncryptionPassphrase(args));
  registerMockCommand("reset_after_forgotten_passphrase", () => resetAfterForgottenPassphrase());
  registerMockCommand("reset_after_startup_failure", () => resetAfterStartupFailure());
  registerMockCommand("get_remember_passphrase_support", () => getRememberPassphraseSupport());
  registerMockCommand("set_remember_passphrase", (args) => setRememberPassphrase(args));
  registerMockCommand("clear_remember_passphrase", () => clearRememberPassphrase());
  registerMockCommand("unlock_with_remembered_passphrase", () => unlockWithRememberedPassphrase());

  registerMockCommand("list_books", () => listBooks());
  registerMockCommand("create_book", (args) => createBook(args));
  registerMockCommand("switch_book", (args) => switchBook(args));
  registerMockCommand("rename_book", (args) => renameBook(args));
  registerMockCommand("remove_book", (args) => removeBook(args));

  registerMockCommand("check_for_updates", () => checkForUpdates());
  registerMockCommand("get_update_surface", () => getUpdateSurface());
  registerMockCommand("download_update", () => downloadUpdate());
  registerMockCommand("install_update", () => installUpdate());

  registerMockCommand("get_sync_status", () => getSyncStatus());
  registerMockCommand("sync_now", (args) => syncNow(args));
  registerMockCommand("get_parked_ops", () => getParkedOps());
  registerMockCommand("get_sync_channel_config", () => getSyncChannelConfig());
  registerMockCommand("set_sync_channel_config", (args) => setSyncChannelConfig(args));
  registerMockCommand("test_sync_channel_connection", (args) => testSyncChannelConnection(args));
  registerMockCommand("get_sync_channel_checkpoint", () => getSyncChannelCheckpoint());
  registerMockCommand("publish_sync_checkpoint", (args) => publishSyncCheckpoint(args));
  registerMockCommand("bootstrap_sync_from_channel", (args) => bootstrapSyncFromChannel(args));

  registerMockCommand("create_backup", (args) => createBackup(args));
  registerMockCommand("restore_backup", (args) => restoreBackup(args));
  registerMockCommand("get_backup_meta", (args) => getBackupMeta(args));
  registerMockCommand("list_backups", (args) => listBackups(args));
  registerMockCommand("get_book_fingerprint", () => getBookFingerprint());
  registerMockCommand("get_device_id", () => getDeviceId());
  registerMockCommand("prune_backups", (args) => pruneBackups(args));
  registerMockCommand("set_auto_backup_dir", (args) => setAutoBackupDir(args));
  registerMockCommand("get_auto_backup_state", () => getAutoBackupState());
  registerMockCommand("set_auto_backup_enabled", (args) => setAutoBackupEnabled(args));

  registerMockCommand("get_data_location_info", () => getDataLocationInfo());
  registerMockCommand("submit_data_location_change", (args) => submitDataLocationChange(args));
  registerMockCommand("restore_default_data_location", (args) => restoreDefaultDataLocation(args));

  registerMockCommand("list_currencies", () => listCurrencies());
  registerMockCommand("list_accounts", () => listAccounts());
  registerMockCommand("list_categories", (args) => listCategories(args));
  registerMockCommand("list_merchants", (args) => listMerchants(args));
  registerMockCommand("list_insurers", (args) => listInsurers(args));
  registerMockCommand("list_account_balances", () => listAccountBalances());

  registerMockCommand("create_category", (args) => createCategory(args));
  registerMockCommand("update_category", (args) => updateCategory(args));
  registerMockCommand("reorder_categories", (args) => reorderCategories(args));
  registerMockCommand("delete_category", (args) => deleteCategory(args));
  registerMockCommand("create_merchant", (args) => createMerchant(args));
  registerMockCommand("update_merchant", (args) => updateMerchant(args));
  registerMockCommand("delete_merchant", (args) => deleteMerchant(args));
  registerMockCommand("create_insurer", (args) => createInsurer(args));
  registerMockCommand("update_insurer", (args) => updateInsurer(args));
  registerMockCommand("delete_insurer", (args) => deleteInsurer(args));
  registerMockCommand("create_account", (args) => createAccount(args));
  registerMockCommand("update_account", (args) => updateAccount(args));
  registerMockCommand("archive_account", (args) => archiveAccount(args));
  registerMockCommand("unarchive_account", (args) => unarchiveAccount(args));
  registerMockCommand("delete_account", (args) => deleteAccount(args));

  registerMockCommand("list_transactions", (args) => listTransactions(args));
  registerMockCommand("create_transaction", (args) => createTransaction(args));
  registerMockCommand("update_transaction", (args) => updateTransaction(args));
  registerMockCommand("delete_transaction", (args) => deleteTransaction(args));

  registerMockCommand("list_instruments", (args) => listInstruments(args));
  registerMockCommand("create_instrument", (args) => createInstrument(args));
  registerMockCommand("delete_instrument", (args) => deleteInstrument(args));
  registerMockCommand("add_fund_by_code", (args) => addFundByCode(args));
  registerMockCommand("add_instrument_by_code", (args) => addInstrumentByCode(args));

  registerMockCommand("list_exchange_rates", () => listExchangeRates());
  registerMockCommand("create_exchange_rate", (args) => createExchangeRate(args));
  registerMockCommand("sync_exchange_rates", () => syncExchangeRates());
  registerMockCommand("list_market_prices", () => listMarketPrices());
  registerMockCommand("create_market_price", (args) => createMarketPrice(args));
  registerMockCommand("record_manual_price", (args) => recordManualPrice(args));

  registerMockCommand("list_budgets", () => listBudgets());
  registerMockCommand("create_budget", (args) => createBudget(args));
  registerMockCommand("update_budget", (args) => updateBudget(args));
  registerMockCommand("delete_budget", (args) => deleteBudget(args));
  registerMockCommand("budget_progress", () => budgetProgress());
  registerMockCommand("budget_annual_summary", () => budgetAnnualSummary());

  registerMockCommand("list_investment_transactions", (args) => listInvestmentTransactions(args));
  registerMockCommand("get_transaction_trade", (args) => getTransactionTrade(args));
  registerMockCommand("get_transaction_convert", (args) => getTransactionConvert(args));
  registerMockCommand("get_transaction_split", (args) => getTransactionSplit(args));

  registerMockCommand("list_holdings", () => listHoldings());
  registerMockCommand("investment_overview", () => investmentOverview());
  registerMockCommand("instrument_price_staleness", () => instrumentPriceStaleness());
  registerMockCommand("portfolio_value_trend", (args) => portfolioValueTrend(args));
  registerMockCommand("realized_pnl_summary", (args) => realizedPnlSummary(args));
  registerMockCommand("cumulative_pnl_summary", () => cumulativePnlSummary());
  registerMockCommand("cumulative_pnl_native_total", () => cumulativePnlNativeTotal());
  registerMockCommand("money_weighted_return_summary", () => moneyWeightedReturnSummary());
  registerMockCommand("sync_instrument_info", () => syncInstrumentInfo());

  registerMockCommand("list_physical_assets", (args) => listPhysicalAssets(args));
  registerMockCommand("get_physical_asset", (args) => getPhysicalAsset(args));
  registerMockCommand("create_physical_asset", (args) => createPhysicalAsset(args));
  registerMockCommand("update_physical_asset", (args) => updatePhysicalAsset(args));
  registerMockCommand("update_physical_asset_valuation", (args) =>
    updatePhysicalAssetValuation(args),
  );
  registerMockCommand("dispose_physical_asset", (args) => disposePhysicalAsset(args));
  registerMockCommand("delete_physical_asset", (args) => deletePhysicalAsset(args));

  registerMockCommand("savings_goal_progress", () => savingsGoalProgress());
  registerMockCommand("create_savings_goal", (args) => createSavingsGoal(args));
  registerMockCommand("update_savings_goal", (args) => updateSavingsGoal(args));
  registerMockCommand("archive_savings_goal", (args) => archiveSavingsGoal(args));
  registerMockCommand("unarchive_savings_goal", (args) => unarchiveSavingsGoal(args));
  registerMockCommand("delete_savings_goal", (args) => deleteSavingsGoal(args));

  registerMockCommand("report_date_range", () => reportDateRange());
  registerMockCommand("monthly_summary", (args) => monthlySummary(args));
  registerMockCommand("category_shares", (args) => categoryShares(args));
  registerMockCommand("merchant_shares", (args) => merchantShares(args));

  registerMockCommand("adjust_account_balance", (args) => adjustAccountBalance(args));
  registerMockCommand("audit_balance_cache", () => auditBalanceCache());
  registerMockCommand("list_merchant_transaction_counts", () => listMerchantTransactionCounts());

  registerMockCommand("search_transactions", (args) => searchTransactions(args));
  registerMockCommand("create_transactions", (args) => createTransactions(args));
  registerMockCommand("get_transaction_order_summary", (args) => getTransactionOrderSummary(args));

  registerMockCommand("dashboard_overview", () => dashboardOverview());
  registerMockCommand("financial_freedom", () => financialFreedom());
  registerMockCommand("cross_book_investment_summary", () => crossBookInvestmentSummary());

  registerMockCommand("create_scheduled_transaction", (args) => createScheduledTransaction(args));
  registerMockCommand("list_scheduled_transactions", () => listScheduledTransactions());
  registerMockCommand("get_scheduled_transaction_detail", (args) =>
    getScheduledTransactionDetail(args),
  );
  registerMockCommand("update_scheduled_transaction_status", (args) =>
    updateScheduledTransactionStatus(args),
  );
  registerMockCommand("update_scheduled_subscription", (args) => updateScheduledSubscription(args));
  registerMockCommand("execute_scheduled_occurrence", (args) => executeScheduledOccurrence(args));
  registerMockCommand("expand_scheduled_occurrences", (args) => expandScheduledOccurrences(args));
  registerMockCommand("subscription_spend_overview", () => subscriptionSpendOverview());

  registerMockCommand("list_items", () => listItems());
  registerMockCommand("calculate_item_cost", (args) => calculateItemCost(args));
  registerMockCommand("create_item", (args) => createItem(args));
  registerMockCommand("update_item", (args) => updateItem(args));
  registerMockCommand("dispose_item", (args) => disposeItem(args));
  registerMockCommand("delete_item", (args) => deleteItem(args));

  registerMockCommand("list_policies", () => listPolicies());
  registerMockCommand("list_policy_stats", () => listPolicyStats());
  registerMockCommand("create_policy", (args) => createPolicy(args));
  registerMockCommand("update_policy", (args) => updatePolicy(args));
  registerMockCommand("delete_policy", (args) => deletePolicy(args));
}
