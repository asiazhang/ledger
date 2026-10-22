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
}
