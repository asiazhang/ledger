import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { formatAmount } from "@ledger/money";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { refCurrencies } from "@ledger/test-support/reference-stubs";
import { resetOverlays } from "@ledger/ui-kit/overlayRegistry";
import InvestmentOverviewPanel from "@/investment/InvestmentOverviewPanel.vue";
import { firePricesChanged, resetPricesChangedHandler } from "./prices-changed-mock";
import { makeInvestmentOverview } from "./factories";

// 价格失效信号订阅基座 mock（issue #238 / ADR-0031 决策 3）：捕获订阅回调，
// 测试手动触发模拟后端 emit（与持仓概览、价格过期提示同款共享辅助）。
vi.mock("@/investment/usePricesChanged", async () => {
  const { capturePricesChangedHandler } = await import("./prices-changed-mock");
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  };
});

// 金额断言委托形态：期待值调同一 formatAmount 实现（格式规则唯一归属其专测）。
const cny = refCurrencies[0];

/** 投资概览契约快照：可投资资产 2200 元 = 现金 1000 元 + 持仓市值 1200 元；
 * 投资合计三项（#1537）：总市值 1200 / 持仓收益 200 / 累计收益 350 元。 */
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
  it("显示可投资资产（本位币）与「投资账户现金 / 持仓市值」两腿拆分", async () => {
    const wrapper = await mountPanel();

    expect(wrapper.get('[data-testid="overview-investable-assets-value"]').text()).toBe(
      formatAmount(220_000, cny),
    );
    expect(wrapper.get('[data-testid="overview-currency"]').text()).toContain("CNY");
    expect(wrapper.get('[data-testid="overview-cash-leg"]').text()).toBe(
      `投资账户现金 ${formatAmount(100_000, cny)}`,
    );
    expect(wrapper.get('[data-testid="overview-holdings-leg"]').text()).toBe(
      `持仓市值 ${formatAmount(120_000, cny)}`,
    );
    // 无缺料状态时不显示额外说明
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
    // 同一标签在两处页签的口径差异必须可解释：合计三项的 ⓘ 挂概览 scope 变体句
    // （读屏经 aria 也能听到差异说明，ADR-0130 决策 3）。
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
    // 合计与两腿照常是「已计入」的部分（缺价持仓不以零虚增）
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
    // 缺折算汇率等报错在场时数字与两腿、合计三项都不渲染（不显示半截数字）
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

  it("纯只读：页内无任何写入口或动作入口（唯一交互是口径说明）", async () => {
    const wrapper = await mountPanel();

    // 无同步 / 录价 / 设置预算等既有动作入口的按钮或文案
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("同步标的信息");
    expect(wrapper.text()).not.toContain("录价");
    expect(wrapper.text()).not.toContain("设置预算");
    // 页内唯一按钮 = 各口径说明触发器（读，不写）：可投资资产 + 合计三项各一个
    expect(wrapper.findAll("button").map((b) => b.attributes("data-testid"))).toEqual([
      "overview-investable-assets-info",
      "overview-total-market-value-info",
      "overview-unrealized-pnl-info",
      "overview-cumulative-pnl-info",
    ]);
  });
});
