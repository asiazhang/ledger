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
  archiveAccount,
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
import { reseed } from "./state";
import { registerMockCommand } from "./tauri/core";
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
}
