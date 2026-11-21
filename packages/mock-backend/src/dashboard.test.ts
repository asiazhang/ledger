import { beforeEach, describe, expect, it } from "vitest";
import type {
  CrossBookInvestmentSummary,
  DashboardOverview,
  FinancialFreedomOverview,
} from "@ledger/types";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

beforeEach(() => {
  installMockBackend();
});

function captureLedgerChanged(): () => number {
  let count = 0;
  void listen("ledger:changed", () => {
    count++;
  });
  return () => count;
}

describe("首页净资产全貌：dashboard_overview（三腿独立人算字面量）", () => {
  it("净资产三腿与合计逐位人算：1_410_800 + 385_348_610 + 132_000 = 386_891_410", async () => {
    await expect(invoke<DashboardOverview>("dashboard_overview")).resolves.toEqual({
      native_currency: "CNY",
      net_worth_cents: 386_891_410,
      accounts_balance_cents: 1_410_800,
      holdings_market_value_cents: 385_348_610,
      physical_assets_value_cents: 132_000,
    });
  });

  it("各腿与域页签同一套夹具出数：投资概览两腿、实物资产在持合计可对上", async () => {
    const [dashboard, overview] = await Promise.all([
      invoke<DashboardOverview>("dashboard_overview"),
      invoke<{ holdings_market_value_cents: number; investment_cash_cents: number }>(
        "investment_overview",
      ),
    ]);
    expect(dashboard.holdings_market_value_cents).toBe(overview.holdings_market_value_cents);
    expect(dashboard.accounts_balance_cents).toBeGreaterThanOrEqual(overview.investment_cash_cents);
    const [dashboard2, assets] = await Promise.all([
      invoke<DashboardOverview>("dashboard_overview"),
      invoke<{ holding_total_native_cents: number }>("list_physical_assets", {}),
    ]);
    expect(dashboard2.physical_assets_value_cents).toBe(assets.holding_total_native_cents);
  });
});

describe("财务自由度：financial_freedom（分子分母独立人算字面量）", () => {
  it("分子 = 现金腿 + 持仓腿；分母 = 月预算×12 + 年预算；ratio/覆盖年数一位小数", async () => {
    await expect(invoke<FinancialFreedomOverview>("financial_freedom")).resolves.toEqual({
      ratio: 321.5,
      numerator_cents: 385_848_610,
      denominator_cents: 3_600_000,
      coverage_years: 107.2,
      native_currency: "CNY",
    });
  });

  it("零分母（未设预算）：ratio 与覆盖年数均答 0，分子不回退", async () => {
    await invoke("delete_budget", { id: "mock-bud-food-monthly" });
    await invoke("delete_budget", { id: "mock-bud-transport-yearly" });
    await expect(invoke<FinancialFreedomOverview>("financial_freedom")).resolves.toEqual({
      ratio: 0,
      numerator_cents: 385_848_610,
      denominator_cents: 0,
      coverage_years: 0,
      native_currency: "CNY",
    });
  });
});

describe("跨账本投资汇总：cross_book_investment_summary（单账本 included 行）", () => {
  it("四数 = 活动本投资口径直投影；books 恒单行 included（单账本常量态）", async () => {
    await expect(
      invoke<CrossBookInvestmentSummary>("cross_book_investment_summary"),
    ).resolves.toEqual({
      target_currency: "CNY",
      converted: false,
      market_value_cents: 385_348_610,
      unrealized_pnl_cents: 35_003_610,
      cumulative_pnl_cents: 34_727_080,
      investable_assets_cents: 385_848_610,
      books: [{ id: "mock-book", name: "Mock 账本", status: "included" }],
    });
  });
});

describe("纯读契约：全局聚合三命令不发失效信号", () => {
  it("三命令全调用后 ledger:changed 计数恒 0（信号契约只辖写路径）", async () => {
    const changed = captureLedgerChanged();
    await invoke("dashboard_overview");
    await invoke("financial_freedom");
    await invoke("cross_book_investment_summary");
    expect(changed()).toBe(0);
  });
});
