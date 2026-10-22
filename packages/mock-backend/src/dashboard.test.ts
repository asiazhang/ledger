import { beforeEach, describe, expect, it } from "vitest";
import type {
  CrossBookInvestmentSummary,
  DashboardOverview,
  FinancialFreedomOverview,
} from "@ledger/types";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import { listen } from "./tauri/event";

/**
 * 全局聚合读数域替身行为测试（issue #1976 / spec #1964 / ADR-0143）：接缝 =
 * 应用所见的 tauri 命令面（`installMockBackend` + `invoke`），断言首页净资产
 * 全貌 / 财务自由度 / 跨账本投资汇总三条顶层读命令——删除任一 index.ts 登记
 * 行即未命中报错、本文件变红（接线负向判据，ADR-0087 断言强度）。
 *
 * 期望值全部为独立人算字面量（事实源 = 夹具，与各域页签测试同一套夹具，
 * 改夹具一行、聚合逐位跟随）：
 *
 * **账户余额腿**（listAccountBalances 口径：期初 + 流水直和，排除隐藏账户，
 * 与 ledger.test.ts 各账户余额人算同源）：
 * - cash 50_000 − 006 支出 1_200 = 48_800
 * - bank 1_000_000 + 002 工资 1_200_000 − 003 还卡 300_000 − 101 买入 205_200
 *   + 102 卖出 3_000 + 105 分红 8_500 − 306 保费 600_000 − 307 定投 500_000
 *   = 606_300
 * - ewallet 20_000 − 001 支出 2_500 + 004 退款 1_500 − 005 支出 8_800
 *   − 301..305 会员 2_500 × 5 = −2_300
 * - credit 0 + 003 还卡转入 300_000 − 201/202 分期 1_000 × 2 = 298_000
 * - invest 0 + 307 定投转入 500_000 = 500_000
 * - goal-emergency 期初 300_000（#1975 储蓄目标专属账户，无流水）
 * - 合计 1_750_800
 *
 * **持仓市值腿**（fixtureHoldings native 列直和，缺价行跳过）：
 * - GOLD 275 + ETF 224_885 + 基金 123_450 + 腾讯 385_000_000 = 385_348_610
 *   （国债缺价不计入；与 investment_aggregates.test.ts 概览市值腿同一套夹具）
 *
 * **实物资产腿**（在持行最新估值）：黄金首饰 132_000（2026-01-10 估值行；
 * 笔记本已处置不计入；与 physical-asset.test.ts 在持合计同一套夹具）
 *
 * **净资产** = 1_750_800 + 385_348_610 + 132_000 = 387_231_410
 *
 * **财务自由度**（分子 = 投资账户现金 + 持仓市值；分母 = 月预算 ×12 + 年预算）：
 * - 分子 = 500_000（券商账户，307 定投转入）+ 385_348_610 = 385_848_610
 * - 分母 = 餐饮月预算 200_000 × 12 + 交通年预算 1_200_000 = 3_600_000
 * - ratio = 385_848_610 × 3% ÷ 3_600_000 × 100 = 321.540…% → 321.5（一位小数）
 * - coverage_years = 385_848_610 ÷ 3_600_000 = 107.180… → 107.2（一位小数）
 *
 * **跨账本投资汇总**（单账本常量态，恒一行 included）：
 * - 市值 = 385_348_610；未实现 = 275 + (−115) + 3_450 + 35_000_000 = 35_003_610
 * - 累计 = 未实现 35_003_610 + 已实现 (−285_030) + 分红 8_500 = 34_727_080
 *   （已实现人算：卖出净额 3_000 − 2400 份 × 均价 120 分 − 费 30 = −285_030，
 *   均价 = 基金持仓成本 120_000 ÷ 剩余 1000 份；与 investment_aggregates.test.ts
 *   概览累计收益同一三腿口径）
 * - 可投资资产 = 500_000 + 385_348_610 = 385_848_610
 *
 * 三条命令全为纯读，不发 ledger:changed（ADR-0012 信号契约只辖写路径）。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

/** 订阅 ledger:changed 并返回触发计数（全局聚合域全读命令，恒 0）。 */
function captureLedgerChanged(): () => number {
  let count = 0;
  void listen("ledger:changed", () => {
    count++;
  });
  return () => count;
}

describe("首页净资产全貌：dashboard_overview（三腿独立人算字面量）", () => {
  it("净资产三腿与合计逐位人算：1_750_800 + 385_348_610 + 132_000 = 387_231_410", async () => {
    await expect(invoke<DashboardOverview>("dashboard_overview")).resolves.toEqual({
      native_currency: "CNY",
      net_worth_cents: 387_231_410, // 三腿合计（见文件头人算）
      accounts_balance_cents: 1_750_800, // 48_800 + 606_300 − 2_300 + 298_000 + 500_000 + 300_000
      holdings_market_value_cents: 385_348_610, // 国债缺价跳过
      physical_assets_value_cents: 132_000, // 已处置笔记本不计入
    });
  });

  it("各腿与域页签同一套夹具出数：投资概览两腿、实物资产在持合计可对上", async () => {
    // 首页四卡的持仓腿 / 可投资资产现金腿必须与投资概览页签同数（票面验收）
    const [dashboard, overview] = await Promise.all([
      invoke<DashboardOverview>("dashboard_overview"),
      invoke<{ holdings_market_value_cents: number; investment_cash_cents: number }>(
        "investment_overview",
      ),
    ]);
    expect(dashboard.holdings_market_value_cents).toBe(overview.holdings_market_value_cents);
    expect(dashboard.accounts_balance_cents).toBeGreaterThanOrEqual(overview.investment_cash_cents);
    // 实物资产腿与实物资产页「在持合计」同数（同一估值夹具）
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
      ratio: 321.5, // 385_848_610 × 3% ÷ 3_600_000 × 100 = 321.540…（一位小数）
      numerator_cents: 385_848_610, // 500_000 + 385_348_610（见文件头人算）
      denominator_cents: 3_600_000, // 200_000 × 12 + 1_200_000（#1948 同款口径）
      coverage_years: 107.2, // 385_848_610 ÷ 3_600_000 = 107.180…（一位小数）
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
      target_currency: "CNY", // 折算目标＝活动本本位币
      converted: false, // 单账本、夹具同币直填不折算，无当期汇率折算发生
      market_value_cents: 385_348_610, // 持仓市值合计（缺价行跳过，同文件头人算）
      unrealized_pnl_cents: 35_003_610, // 275 − 115 + 3_450 + 35_000_000
      cumulative_pnl_cents: 34_727_080, // 未实现 + 已实现 (−285_030) + 分红 8_500
      investable_assets_cents: 385_848_610, // 500_000 + 385_348_610
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
