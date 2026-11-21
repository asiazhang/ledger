import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { nextTick } from "vue";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { formatAmount, amountPrivacyEnabled } from "@ledger/money";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import { probeColor } from "@ledger/test-support/dom";
import { pnlSemanticColor } from "@ledger/theme/semantic-colors";
import { refCurrencies } from "@ledger/test-support/reference-stubs";
import { resetOverlays } from "@ledger/ui-kit/overlayRegistry";
import InvestmentOverviewPanel from "@/investment/InvestmentOverviewPanel.vue";
import { firePricesChanged, resetPricesChangedHandler } from "./prices-changed-mock";
import { makeInvestmentOverview } from "./factories";

vi.mock("@/investment/usePricesChanged", async () => {
  const { capturePricesChangedHandler } = await import("./prices-changed-mock");
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  };
});

const cny = refCurrencies[0];

const OVERVIEW = makeInvestmentOverview({
  investable_assets_cents: 220_000,
  investment_cash_cents: 100_000,
  holdings_market_value_cents: 120_000,
  total_market_value_cents: 120_000,
  unrealized_pnl_cents: 20_000,
  cumulative_pnl_cents: 35_000,
});

beforeEach(async () => {
  resetOverlays();
  resetPricesChangedHandler();
  wireInvokeSeam({ defaults: { investment_overview: OVERVIEW } });
  await useReferenceStore().refresh();
});

async function mountPanel(): Promise<ReturnType<typeof mount>> {
  const wrapper = mount(InvestmentOverviewPanel);
  await flushPromises();
  return wrapper;
}

describe("InvestmentOverviewPanel 投资概览（spec #1532 / issue #1536）", () => {
  it("组合走势卡随页签装配（#1907 / ADR-0131 决策 9 修订注记）：卡片在位、挂载即拉组合市值", async () => {
    const wrapper = await mountPanel();
    expect(wrapper.find('[data-testid="overview-trend-card"]').exists()).toBe(true);
    const text = wrapper.text();
    for (const label of ["1 月", "3 月", "1 年", "3 年", "5 年", "全部"]) {
      expect(text).toContain(label);
    }
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "portfolio_value_trend")).toBe(true);
  });

  it("显示可投资资产（本位币）与「投资账户现金 / 持仓市值」两腿拆分", async () => {
    const wrapper = await mountPanel();

    expect(wrapper.get('[data-testid="overview-investable-assets-value"]').text()).toBe(
      formatAmount(220_000, cny),
    );
    expect(wrapper.get('[data-testid="overview-currency"]').text()).toContain("CNY");
    const testIds = wrapper
      .findAll("[data-testid]")
      .map((el) => el.attributes("data-testid") ?? "");
    expect(testIds.indexOf("overview-investable-assets-info")).toBeLessThan(
      testIds.indexOf("overview-currency"),
    );
    expect(testIds.indexOf("overview-currency")).toBeLessThan(
      testIds.indexOf("overview-investable-assets-value"),
    );
    expect(wrapper.get('[data-testid="overview-cash-leg"]').text()).toBe(
      `投资账户现金 ${formatAmount(100_000, cny)}`,
    );
    expect(wrapper.get('[data-testid="overview-holdings-leg"]').text()).toBe(
      `持仓市值 ${formatAmount(120_000, cny)}`,
    );
    expect(wrapper.find('[data-testid="overview-missing-price"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="overview-no-account"]').exists()).toBe(false);
  });

  it("投资合计三项与可投资资产同页可读（总市值 / 持仓收益 / 累计收益，均折本位币）", async () => {
    const wrapper = await mountPanel();

    expect(wrapper.get('[data-testid="overview-totals-title"]').text()).toBe("投资合计");
    expect(wrapper.get('[data-testid="overview-total-market-value"]').text()).toContain("总市值");
    expect(wrapper.get('[data-testid="overview-total-market-value-value"]').text()).toBe(
      formatAmount(120_000, cny),
    );
    expect(wrapper.get('[data-testid="overview-unrealized-pnl"]').text()).toContain("持仓收益");
    expect(wrapper.get('[data-testid="overview-unrealized-pnl-value"]').text()).toBe(
      formatAmount(20_000, cny),
    );
    expect(wrapper.get('[data-testid="overview-cumulative-pnl"]').text()).toContain("累计收益");
    expect(wrapper.get('[data-testid="overview-cumulative-pnl-value"]').text()).toBe(
      formatAmount(35_000, cny),
    );
    for (const [id, label] of [
      ["overview-total-market-value", t("investments.concepts.marketValue")],
      ["overview-unrealized-pnl", t("investments.concepts.unrealizedPnl")],
      ["overview-cumulative-pnl", t("investments.concepts.cumulativePnl")],
    ] as const) {
      const wrapper = await mountPanel();
      expect(wrapper.get(`[data-testid="${id}-info"]`).attributes("aria-label")).toBe(
        t("investments.concepts.tipAria", { concept: label }),
      );
      wrapper.unmount();
    }
  });

  it("缺现价持仓不计入但显式给出未计入数量说明（不静默低估）", async () => {
    wireInvokeSeam({
      defaults: { investment_overview: { ...OVERVIEW, missing_price_holding_count: 2 } },
    });
    const wrapper = await mountPanel();

    expect(wrapper.get('[data-testid="overview-missing-price"]').text()).toBe(
      "另有 2 只持仓因缺现价或折算汇率未计入。",
    );
    expect(wrapper.get('[data-testid="overview-investable-assets-value"]').text()).toBe(
      formatAmount(220_000, cny),
    );
  });

  it("没有投资账户：显示 0 并给一句引导", async () => {
    wireInvokeSeam({
      defaults: {
        investment_overview: makeInvestmentOverview({ has_investment_account: false }),
      },
    });
    const wrapper = await mountPanel();

    expect(wrapper.get('[data-testid="overview-investable-assets-value"]').text()).toBe(
      formatAmount(0, cny),
    );
    expect(wrapper.get('[data-testid="overview-no-account"]').text()).toContain("还没有投资账户");
  });

  it("缺折算汇率：卡内警告 + 重试，不显示半截数字；重试成功后数字上屏", async () => {
    let calls = 0;
    wireInvokeSeam({
      defaults: {},
      overrides: {
        investment_overview: () =>
          calls++ === 0
            ? Promise.reject(new Error("未找到 USD -> CNY 的汇率（正反向均无）"))
            : OVERVIEW,
      },
    });
    const wrapper = await mountPanel();

    const alert = wrapper.get('[data-testid="overview-error"]');
    expect(alert.text()).toContain("汇率");
    expect(wrapper.find('[data-testid="overview-investable-assets-value"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="overview-cash-leg"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="overview-total-market-value-value"]').exists()).toBe(false);

    await wrapper.get('[data-testid="overview-retry"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="overview-error"]').exists()).toBe(false);
    expect(wrapper.get('[data-testid="overview-investable-assets-value"]').text()).toBe(
      formatAmount(220_000, cny),
    );
  });

  it("价格失效信号后自动重拉：写价后的翻新数字上屏（删除接线即变红）", async () => {
    let calls = 0;
    wireInvokeSeam({
      defaults: {},
      overrides: {
        investment_overview: () =>
          calls++ === 0
            ? OVERVIEW
            : {
                ...OVERVIEW,
                investable_assets_cents: 300_000,
                holdings_market_value_cents: 200_000,
              },
      },
    });
    const wrapper = await mountPanel();
    expect(wrapper.get('[data-testid="overview-investable-assets-value"]').text()).toBe(
      formatAmount(220_000, cny),
    );

    firePricesChanged();
    await flushPromises();

    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "investment_overview")).toHaveLength(2);
    expect(wrapper.get('[data-testid="overview-holdings-leg"]').text()).toBe(
      `持仓市值 ${formatAmount(200_000, cny)}`,
    );
    expect(wrapper.text()).not.toContain(formatAmount(220_000, cny));
  });

  it("构成比例条：两段宽度 = 各腿 ÷ 可投资资产、和恒 100%，比例条对辅助技术隐藏（spec #1684）", async () => {
    const wrapper = await mountPanel();

    const bar = wrapper.get('[data-testid="overview-composition-bar"]');
    expect(bar.attributes("aria-hidden")).toBe("true");
    const pctOf = (testId: string) =>
      parseFloat((wrapper.get(`[data-testid="${testId}"]`).element as HTMLElement).style.width);
    const cashPct = pctOf("overview-bar-cash");
    const holdingsPct = pctOf("overview-bar-holdings");
    expect(cashPct).toBeCloseTo(45.454545455, 6);
    expect(holdingsPct).toBeCloseTo(54.545454545, 6);
    expect(cashPct + holdingsPct).toBeCloseTo(100, 6);
    expect(wrapper.get('[data-testid="overview-cash-leg"]').text()).toBe(
      `投资账户现金 ${formatAmount(100_000, cny)}`,
    );
    expect(wrapper.get('[data-testid="overview-holdings-leg"]').text()).toBe(
      `持仓市值 ${formatAmount(120_000, cny)}`,
    );
  });

  it("可投资资产为 0：不出现空白比例条，金额照常显 0、图例照常可读（spec #1684）", async () => {
    wireInvokeSeam({ defaults: { investment_overview: makeInvestmentOverview() } });
    const wrapper = await mountPanel();

    expect(wrapper.find('[data-testid="overview-composition-bar"]').exists()).toBe(false);
    expect(wrapper.get('[data-testid="overview-investable-assets-value"]').text()).toBe(
      formatAmount(0, cny),
    );
    expect(wrapper.get('[data-testid="overview-cash-leg"]').text()).toBe(
      `投资账户现金 ${formatAmount(0, cny)}`,
    );
    expect(wrapper.get('[data-testid="overview-holdings-leg"]').text()).toBe(
      `持仓市值 ${formatAmount(0, cny)}`,
    );
  });

  it("金额隐私模式：比例条一并隐藏（相对构成也不泄露），金额照常走统一掩码（spec #1684）", async () => {
    const wrapper = await mountPanel();
    amountPrivacyEnabled.value = true;
    await nextTick();

    expect(wrapper.find('[data-testid="overview-composition-bar"]').exists()).toBe(false);
    expect(wrapper.get('[data-testid="overview-investable-assets-value"]').text()).toBe("••••");
    expect(wrapper.get('[data-testid="overview-cash-leg"]').text()).toBe("投资账户现金 ••••");
    expect(wrapper.get('[data-testid="overview-holdings-leg"]').text()).toBe("持仓市值 ••••");
  });

  it("三合计一行细条：持仓收益与累计收益取盈亏涨跌色（红涨绿跌），总市值保持中性", async () => {
    wireInvokeSeam({
      defaults: {
        investment_overview: makeInvestmentOverview({
          investable_assets_cents: 220_000,
          investment_cash_cents: 100_000,
          holdings_market_value_cents: 120_000,
          total_market_value_cents: 120_000,
          unrealized_pnl_cents: 20_000,
          cumulative_pnl_cents: -35_000,
        }),
      },
    });
    const wrapper = await mountPanel();
    const theme = useAppStore().theme;
    const colorOf = (testId: string) =>
      (wrapper.get(`[data-testid="${testId}-value"]`).element as HTMLElement).style.color;
    expect(colorOf("overview-total-market-value")).toBe("");
    expect(colorOf("overview-investable-assets")).toBe("");
    expect(colorOf("overview-unrealized-pnl")).toBe(probeColor(pnlSemanticColor(20_000, theme)));
    expect(colorOf("overview-cumulative-pnl")).toBe(probeColor(pnlSemanticColor(-35_000, theme)));
  });

  it("纯只读：页内无任何写入口或动作入口（唯一交互是口径说明；#1907 起走势卡同规）", async () => {
    const wrapper = await mountPanel();

    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("同步标的信息");
    expect(wrapper.text()).not.toContain("录价");
    expect(wrapper.text()).not.toContain("设置预算");
    expect(wrapper.findAll("button").map((b) => b.attributes("data-testid"))).toEqual([
      "overview-investable-assets-info",
      "overview-total-market-value-info",
      "overview-unrealized-pnl-info",
      "overview-cumulative-pnl-info",
      "trend-concept-info",
    ]);
  });
});
