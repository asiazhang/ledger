import type {
  CrossBookInvestmentSummary,
  DashboardOverview,
  FinancialFreedomOverview,
} from "@ledger/types";
import { MOCK_BOOK } from "./books";
import { budgetAnnualSummary } from "./budget";
import { listAccountBalances } from "./ledger";
import { cumulativePnlNativeTotal, investmentOverview } from "./investment_aggregates";
import { listPhysicalAssets } from "./physical-asset";
import { db } from "./state";

/**
 * 全局聚合读数域替身（issue #1976 / spec #1964 / ADR-0143）：首页净资产全貌、
 * 财务自由度与跨账本投资汇总三条顶层读命令——首页四卡与投资概览各腿在浏览器
 * 出数，各腿数字由已落地的域夹具直投影汇合。刻意不追行为等价（ADR-0143 决策 1）：
 *
 * - **聚合口径复用各域模块导出，不另起第二套**（spec 取向）：余额腿 =
 *   listAccountBalances（ledger.ts 唯一余额口径，排除隐藏账户、含投资账户现金，
 *   ADR-0020 修订）；持仓腿 = investmentOverview 的市值腿（缺价行跳过）；
 *   实物资产腿 = listPhysicalAssets 的在持合计（已处置 / 软删不计入）；
 *   财务自由度分子 = investmentOverview 的可投资资产合计（投资账户现金 +
 *   持仓市值两腿）；分母 = budgetAnnualSummary 的年度预算总额（月 ×12 + 年，
 *   #1948 与预算页年度汇总卡同源单点）；跨账本汇总四数 = 投资概览三腿 +
 *   累计收益三腿合计。真后端的跨域读快照（ensure_transaction）、余额缓存指纹
 *   与读探针、缺汇率码化上抛都不模拟。
 * - 全部纯读，不发失效信号（ADR-0012 信号契约只辖写路径）。
 */

/** 首页净资产全貌：真实财富视角三腿合计（账户余额 + 持仓市值 + 在持实物资产）。 */
export function dashboardOverview(): DashboardOverview {
  const accounts = listAccountBalances().reduce((sum, b) => sum + b.balance_cents, 0);
  const holdings = investmentOverview().holdings_market_value_cents;
  const physical = listPhysicalAssets({}).holding_total_native_cents;
  return {
    native_currency: db().settings.base_currency,
    net_worth_cents: accounts + holdings + physical,
    accounts_balance_cents: accounts,
    holdings_market_value_cents: holdings,
    physical_assets_value_cents: physical,
  };
}

/** 一位小数四舍五入（ratio 与 coverage_years 共用刻度，真后端 round1 同形）。 */
function round1(value: number): number {
  return Math.round(value * 10) / 10;
}

/** 财务自由度：可投资资产 × 3% 安全提取率对年度预算总额的覆盖比例（ADR-0048）；
 * 零分母（未设预算）ratio 与覆盖年数均答 0，真后端同语义。 */
export function financialFreedom(): FinancialFreedomOverview {
  const numerator = investmentOverview().investable_assets_cents;
  const denominator = budgetAnnualSummary().annual_budget_total_cents;
  const ratio = denominator === 0 ? 0 : round1(((numerator * 0.03) / denominator) * 100);
  const coverageYears = denominator === 0 ? 0 : round1(numerator / denominator);
  return {
    ratio,
    numerator_cents: numerator,
    denominator_cents: denominator,
    coverage_years: coverageYears,
    native_currency: db().settings.base_currency,
  };
}

/** 跨账本投资汇总（ADR-0114）：替身恒单账本常量态（books.ts）——单行 included、
 * 无跨本合并与当期汇率折算（converted 恒 false），四数即活动本投资口径直投影。 */
export function crossBookInvestmentSummary(): CrossBookInvestmentSummary {
  const overview = investmentOverview();
  return {
    target_currency: db().settings.base_currency,
    converted: false,
    market_value_cents: overview.holdings_market_value_cents,
    unrealized_pnl_cents: overview.unrealized_pnl_cents,
    cumulative_pnl_cents: cumulativePnlNativeTotal().total_cents,
    investable_assets_cents: overview.investable_assets_cents,
    books: [{ id: MOCK_BOOK.id, name: MOCK_BOOK.name, status: "included" }],
  };
}
