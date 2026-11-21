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

function round1(value: number): number {
  return Math.round(value * 10) / 10;
}

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
