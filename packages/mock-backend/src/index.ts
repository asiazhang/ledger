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

/**
 * 替身后端安装入口（issue #1962 / ADR-0143 决策 4）：main.ts 按 vite mode 静态
 * 分支调用——`pnpm dev:mock`（--mode mock）下安装命令表并播种夹具；生产构建
 * 本分支恒假、动态 import 被静态消除，替身不进生产包（接线由
 * src/__tests__/mock-mode-wiring.test.ts 守门，删除即变红）。
 *
 * 可重复安装：重装即重播种（状态整体回夹具种子），命令表按名覆盖——与「刷新
 * 页面状态重置」同一语义，HMR 重执行 bootstrap 亦收敛。
 */
export function installMockBackend(): void {
  reseed();

  // 启动门（ADR-0143 决策 6）：答「已解锁」直接进主界面
  registerMockCommand("get_boot_status", () => getBootStatus());

  // 启动链路与设置（issue #1965）：开关推送 no-op、本位币/日志等级内存态、
  // 构建信息与 AI 提示字面量、壳命令 no-op——启动推送不弹错，设置页「通用」
  // 「关于」取数可用
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

  // 加密与记住主口令（issue #1966）：环境壳常量态——mock 库恒已解锁明文，
  // 状态恒「明文库未锁定」、转换/重置 void、记住口令按不支持平台形态应答
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

  // 账本注册表（issue #1966）：单账本常量态——清单恒一本可变更无回退，
  // 新建/切换/改名/移除答形状正确的常量回执，不真正改注册表
  registerMockCommand("list_books", () => listBooks());
  registerMockCommand("create_book", (args) => createBook(args));
  registerMockCommand("switch_book", (args) => switchBook(args));
  registerMockCommand("rename_book", (args) => renameBook(args));
  registerMockCommand("remove_book", (args) => removeBook(args));

  // 应用更新（issue #1966）：已是最新常量态——检查 same、入口 updater、
  // 下载占位版本、安装 void，不发进度事件
  registerMockCommand("check_for_updates", () => checkForUpdates());
  registerMockCommand("get_update_surface", () => getUpdateSurface());
  registerMockCommand("download_update", () => downloadUpdate());
  registerMockCommand("install_update", () => installUpdate());

  // 多端同步（issue #1966）：未配置常量态——状态无挂起明文库、轮次零计数、
  // 挂起清单空、通道空表单读写、预检 null、发布与引导占位回执
  registerMockCommand("get_sync_status", () => getSyncStatus());
  registerMockCommand("sync_now", (args) => syncNow(args));
  registerMockCommand("get_parked_ops", () => getParkedOps());
  registerMockCommand("get_sync_channel_config", () => getSyncChannelConfig());
  registerMockCommand("set_sync_channel_config", (args) => setSyncChannelConfig(args));
  registerMockCommand("test_sync_channel_connection", (args) => testSyncChannelConnection(args));
  registerMockCommand("get_sync_channel_checkpoint", () => getSyncChannelCheckpoint());
  registerMockCommand("publish_sync_checkpoint", (args) => publishSyncCheckpoint(args));
  registerMockCommand("bootstrap_sync_from_channel", (args) => bootstrapSyncFromChannel(args));

  // 备份（issue #1966）：空占位/常量态——创建恢复占位回执、元数据明文手动、
  // 清单空修剪零动作、自动备份默认开启从未备份、推送 void（不发备份信号）
  registerMockCommand("create_backup", (args) => createBackup(args));
  registerMockCommand("restore_backup", (args) => restoreBackup(args));
  registerMockCommand("get_backup_meta", (args) => getBackupMeta(args));
  registerMockCommand("list_backups", (args) => listBackups(args));
  registerMockCommand("prune_backups", (args) => pruneBackups(args));
  registerMockCommand("set_auto_backup_dir", (args) => setAutoBackupDir(args));
  registerMockCommand("get_auto_backup_state", () => getAutoBackupState());
  registerMockCommand("set_auto_backup_enabled", (args) => setAutoBackupEnabled(args));

  // 数据位置（issue #1966）：默认目录无挂起常量态，更改/恢复答已落盘回执
  registerMockCommand("get_data_location_info", () => getDataLocationInfo());
  registerMockCommand("submit_data_location_change", (args) => submitDataLocationChange(args));
  registerMockCommand("restore_default_data_location", (args) => restoreDefaultDataLocation(args));

  // 参考数据五字典：读
  registerMockCommand("list_currencies", () => listCurrencies());
  registerMockCommand("list_accounts", () => listAccounts());
  registerMockCommand("list_categories", (args) => listCategories(args));
  registerMockCommand("list_merchants", (args) => listMerchants(args));
  registerMockCommand("list_insurers", (args) => listInsurers(args));
  registerMockCommand("list_account_balances", () => listAccountBalances());

  // 参考数据五字典：写（完成后统一发 ledger:changed，见 ledger.ts）
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

  // 交易读写闭环
  registerMockCommand("list_transactions", (args) => listTransactions(args));
  registerMockCommand("create_transaction", (args) => createTransaction(args));
  registerMockCommand("update_transaction", (args) => updateTransaction(args));
  registerMockCommand("delete_transaction", (args) => deleteTransaction(args));

  // 投资标的字典（issue #1969）：投资页「标的」页签读投影（四维筛选 + 分页
  // + anchor 定位）与自建/按代码添加写闭环（完成后统一发 ledger:changed）
  registerMockCommand("list_instruments", (args) => listInstruments(args));
  registerMockCommand("create_instrument", (args) => createInstrument(args));
  registerMockCommand("delete_instrument", (args) => deleteInstrument(args));
  registerMockCommand("add_fund_by_code", (args) => addFundByCode(args));
  registerMockCommand("add_instrument_by_code", (args) => addInstrumentByCode(args));

  // 汇率与行情价格（issue #1972）：设置页汇率卡与投资页录价读写闭环——
  // 汇率按货币对 upsert 写后发 ledger:changed，同步答常量报告不发进度事件；
  // 现价缓存 per-instrument upsert、手动报价两落点写入（历史周采样 + 最新点
  // 映像规则现价），价格写入发 ledger:prices-changed
  registerMockCommand("list_exchange_rates", () => listExchangeRates());
  registerMockCommand("create_exchange_rate", (args) => createExchangeRate(args));
  registerMockCommand("sync_exchange_rates", () => syncExchangeRates());
  registerMockCommand("list_market_prices", () => listMarketPrices());
  registerMockCommand("create_market_price", (args) => createMarketPrice(args));
  registerMockCommand("record_manual_price", (args) => recordManualPrice(args));

  // 预算读写闭环（issue #1967）：CRUD 内存态 + 进度/年度汇总按交易夹具直聚合，
  // 写完成后统一发 ledger:changed（见 budget.ts 头注的从简口径）
  registerMockCommand("list_budgets", () => listBudgets());
  registerMockCommand("create_budget", (args) => createBudget(args));
  registerMockCommand("update_budget", (args) => updateBudget(args));
  registerMockCommand("delete_budget", (args) => deleteBudget(args));
  registerMockCommand("budget_progress", () => budgetProgress());
  registerMockCommand("budget_annual_summary", () => budgetAnnualSummary());

  // 投资明细与交易扩展投影（issue #1970）：投资页「明细」页签读投影（五种投资
  // kind 行 JOIN 证券扩展行，四维筛选 + offset 分页）与编辑回填三投影（非对应
  // kind 答错）——纯读命令，不发失效信号（见 investment_txns.ts 头注）
  registerMockCommand("list_investment_transactions", (args) => listInvestmentTransactions(args));
  registerMockCommand("get_transaction_trade", (args) => getTransactionTrade(args));
  registerMockCommand("get_transaction_convert", (args) => getTransactionConvert(args));
  registerMockCommand("get_transaction_split", (args) => getTransactionSplit(args));

  // 投资聚合读数（issue #1971）：持仓 / 概览 / 走势 / 盈亏系列页签取数——
  // 聚合按夹具直投影给合理单值（不追行为等价，见 investment_aggregates.ts
  // 头注）；除 sync_instrument_info 发 ledger:prices-changed 外全部纯读
  registerMockCommand("list_holdings", () => listHoldings());
  registerMockCommand("investment_overview", () => investmentOverview());
  registerMockCommand("instrument_price_staleness", () => instrumentPriceStaleness());
  registerMockCommand("portfolio_value_trend", (args) => portfolioValueTrend(args));
  registerMockCommand("realized_pnl_summary", (args) => realizedPnlSummary(args));
  registerMockCommand("cumulative_pnl_summary", () => cumulativePnlSummary());
  registerMockCommand("cumulative_pnl_native_total", () => cumulativePnlNativeTotal());
  registerMockCommand("money_weighted_return_summary", () => moneyWeightedReturnSummary());
  registerMockCommand("sync_instrument_info", () => syncInstrumentInfo());

  // 实物资产（issue #1975）：估值档案内存态——估值只追加、当前估值 = 最新一条，
  // 在持合计恒指在持；写完成后统一发 ledger:changed（见 physical-asset.ts 头注）
  registerMockCommand("list_physical_assets", (args) => listPhysicalAssets(args));
  registerMockCommand("get_physical_asset", (args) => getPhysicalAsset(args));
  registerMockCommand("create_physical_asset", (args) => createPhysicalAsset(args));
  registerMockCommand("update_physical_asset", (args) => updatePhysicalAsset(args));
  registerMockCommand("update_physical_asset_valuation", (args) =>
    updatePhysicalAssetValuation(args),
  );
  registerMockCommand("dispose_physical_asset", (args) => disposePhysicalAsset(args));
  registerMockCommand("delete_physical_asset", (args) => deletePhysicalAsset(args));

  // 储蓄目标（issue #1975）：创建同建专属账户（other 类型、1:1 绑定、账户名
  // 随动）、进度 = 专属账户余额直和 + ETA/所需月存双向推算；写完成后统一发
  // ledger:changed（见 savings-goal.ts 头注）
  registerMockCommand("savings_goal_progress", () => savingsGoalProgress());
  registerMockCommand("create_savings_goal", (args) => createSavingsGoal(args));
  registerMockCommand("update_savings_goal", (args) => updateSavingsGoal(args));
  registerMockCommand("archive_savings_goal", (args) => archiveSavingsGoal(args));
  registerMockCommand("unarchive_savings_goal", (args) => unarchiveSavingsGoal(args));
  registerMockCommand("delete_savings_goal", (args) => deleteSavingsGoal(args));

  // 报表聚合（issue #1968）：日期极值 / 月度汇总 / 分类份额 / 商户排行按交易夹具
  // 直聚合（从简口径见 reports.ts 头注）；全读命令不发 ledger:changed。
  registerMockCommand("report_date_range", () => reportDateRange());
  registerMockCommand("monthly_summary", (args) => monthlySummary(args));
  registerMockCommand("category_shares", (args) => categoryShares(args));
  registerMockCommand("merchant_shares", (args) => merchantShares(args));

  // 账户/商户扩展（issue #1968）：余额校准黑洞转账（发 ledger:changed）、
  // 缓存审计答无漂移（读）、商户毛笔数计数（读）。
  registerMockCommand("adjust_account_balance", (args) => adjustAccountBalance(args));
  registerMockCommand("audit_balance_cache", () => auditBalanceCache());
  registerMockCommand("list_merchant_transaction_counts", () => listMerchantTransactionCounts());

  // 交易读扩展（issue #1968）：搜索（扁平 camelCase 载荷）+ 批量创建（ledger:changed
  // 整批只发一次）+ 订单汇总；get_transaction_trade/convert/split 已由 #1970 落地。
  registerMockCommand("search_transactions", (args) => searchTransactions(args));
  registerMockCommand("create_transactions", (args) => createTransactions(args));
  registerMockCommand("get_transaction_order_summary", (args) => getTransactionOrderSummary(args));

  // 全局聚合页（issue #1976）：首页净资产全貌 / 财务自由度 / 跨账本投资汇总
  // 三条顶层读命令，各腿由已落地域夹具直投影汇合（口径复用各域模块导出，
  // 见 dashboard.ts 头注）；全读命令不发 ledger:changed。
  registerMockCommand("dashboard_overview", () => dashboardOverview());
  registerMockCommand("financial_freedom", () => financialFreedom());
  registerMockCommand("cross_book_investment_summary", () => crossBookInvestmentSummary());

  // 定时/订阅（issue #1973）：计划 + 期次内存态读写闭环——建计划展开期次、
  // 手动执行真落交易（订阅/分期 expense、转账 transfer）、订阅花费聚合；
  // 写完成后统一发 ledger:changed（见 scheduled.ts 头注）
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

  // 物品（issue #1974）：档案内存态 CRUD + 处置重锚 + 日均成本三元组（含起止
  // 两端、分子扣残值下限 0，在用摊替身今日 / 已处置摊处置日）；写完成后统一
  // 发 ledger:changed（见 item.ts 头注的从简口径）
  registerMockCommand("list_items", () => listItems());
  registerMockCommand("calculate_item_cost", (args) => calculateItemCost(args));
  registerMockCommand("create_item", (args) => createItem(args));
  registerMockCommand("update_item", (args) => updateItem(args));
  registerMockCommand("dispose_item", (args) => disposeItem(args));
  registerMockCommand("delete_item", (args) => deleteItem(args));

  // 保单（issue #1974）：档案内存态 CRUD + 逐保单统计（保费/流入挂单直和、
  // 下期扣款从挂靠协议 pending 期次推导、到期态实时推导）；写完成后统一发
  // ledger:changed（见 policy.ts 头注的从简口径）
  registerMockCommand("list_policies", () => listPolicies());
  registerMockCommand("list_policy_stats", () => listPolicyStats());
  registerMockCommand("create_policy", (args) => createPolicy(args));
  registerMockCommand("update_policy", (args) => updatePolicy(args));
  registerMockCommand("delete_policy", (args) => deletePolicy(args));
}
