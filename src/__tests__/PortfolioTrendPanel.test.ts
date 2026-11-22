import { describe, it, expect, vi, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { useReferenceStore } from "@/stores/reference";
import PortfolioTrendPanel from "@/investment/PortfolioTrendPanel.vue";
import { hoverTipText } from "@ledger/test-support/tooltip";
import { firePricesChanged, resetPricesChangedHandler } from "./prices-changed-mock";
import type { PortfolioValueTrend } from "@ledger/types";

vi.mock("vue-chartjs", async () => {
  const { LineChartStub } = await import("./line-chart-stub");
  return { Line: LineChartStub };
});

vi.mock("@/investment/usePricesChanged", async () => {
  const { capturePricesChangedHandler } = await import("./prices-changed-mock");
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  };
});

const portfolioTrend: PortfolioValueTrend = {
  currency_code: "CNY",
  points: [
    { date: "2026-06-05", market_value_cents: 100000 },
    { date: "2026-06-12", market_value_cents: 110000 },
  ],
};

const portfolioTrendResponse = () => Promise.resolve(portfolioTrend);

beforeEach(async () => {
  resetPricesChangedHandler();
  wireInvokeSeam({
    overrides: { portfolio_value_trend: portfolioTrendResponse },
  });
  const store = useReferenceStore();
  await store.refresh();
});

function chartPayload(wrapper: ReturnType<typeof mount>): {
  labels: string[];
  datasets: { label?: string; data: number[] }[];
} {
  const el = wrapper.get('[data-testid="line-chart"]');
  return JSON.parse(el.text());
}

describe("PortfolioTrendPanel 走势卡（#1907 起随概览页签）", () => {
  it("挂载即拉组合市值并渲染图表序列与本位币标注", async () => {
    const wrapper = mount(PortfolioTrendPanel);
    await flushPromises();
    const call = mockInvoke.mock.calls.find(([c]) => c === "portfolio_value_trend");
    expect(call).toBeTruthy();
    const payload = chartPayload(wrapper);
    expect(payload.labels).toEqual(["2026-06-05", "2026-06-12"]);
    expect(payload.datasets[0].data).toEqual([100000, 110000]);
    expect(wrapper.get('[data-testid="trend-currency"]').text()).toContain("CNY");
    expect(wrapper.get('[data-testid="trend-currency"]').text()).toContain("本位币");
  });

  it("渲染预设区间档位闭集（1 月 / 3 月 / 1 年 / 3 年 / 5 年 / 全部，issue #1907）", async () => {
    const wrapper = mount(PortfolioTrendPanel);
    await flushPromises();
    const text = wrapper.text();
    for (const label of ["1 月", "3 月", "1 年", "3 年", "5 年", "全部"]) {
      expect(text).toContain(label);
    }
    expect(text).not.toContain("单标的");
  });

  it("预设区间切换后重新查询并携带新的起始日期", async () => {
    const wrapper = mount(PortfolioTrendPanel);
    await flushPromises();
    const before = mockInvoke.mock.calls.filter(([c]) => c === "portfolio_value_trend").length;
    const radio = wrapper.findAll(".n-radio").find((r) => r.text() === "1 月");
    expect(radio).toBeTruthy();
    await radio!.find("input").setValue(true);
    await flushPromises();
    const after = mockInvoke.mock.calls.filter(([c]) => c === "portfolio_value_trend").length;
    expect(after).toBeGreaterThan(before);
    const last = mockInvoke.mock.calls.filter(([c]) => c === "portfolio_value_trend").at(-1)!;
    expect((last[1] as { filter: { start_date: string } }).filter.start_date).toBeTruthy();
  });

  it("组合走势无数据且无补全状态 → 中性空态，不出现指向同步按钮的回填文案", async () => {
    wireInvokeSeam({
      overrides: {
        portfolio_value_trend: { currency_code: "CNY", points: [] },
      },
    });
    const wrapper = mount(PortfolioTrendPanel);
    await flushPromises();
    expect(wrapper.text()).toContain("暂无历史价格数据");
    expect(wrapper.text()).not.toContain("同步标的信息");
    expect(wrapper.text()).toContain("更长的区间");
    expect(wrapper.find('[data-testid="line-chart"]').exists()).toBe(false);
  });

  it("组合走势无数据且库内有待补全标的 → 空态三态：补全中（带计数）", async () => {
    wireInvokeSeam({
      overrides: {
        portfolio_value_trend: {
          currency_code: "CNY",
          points: [],
          backfill: { state: "running", done: 13, total: 228 },
        },
      },
    });
    const wrapper = mount(PortfolioTrendPanel);
    await flushPromises();
    const extra = wrapper.get('[data-testid="trend-empty-backfill"]');
    expect(extra.text()).toContain("补全中");
    expect(extra.text()).toContain("13/228");
    expect(wrapper.text()).not.toContain("同步标的信息");
  });

  it("价格失效信号触发后重拉走势：键（区间）未变也强制重取（issue #238）", async () => {
    const wrapper = mount(PortfolioTrendPanel);
    await flushPromises();
    const before = mockInvoke.mock.calls.filter(([c]) => c === "portfolio_value_trend").length;
    firePricesChanged();
    await flushPromises();
    const calls = mockInvoke.mock.calls.filter(([c]) => c === "portfolio_value_trend");
    expect(calls.length).toBe(before + 1);
    expect(chartPayload(wrapper).datasets[0].data).toEqual([100000, 110000]);
  });

  it("曲线口径说明：组合市值概念的常驻 ⓘ（issue #1369）", async () => {
    const wrapper = mount(PortfolioTrendPanel);
    await flushPromises();
    const trigger = wrapper.find('[data-testid="trend-concept-info"]');
    expect(trigger.exists()).toBe(true);
    expect(trigger.attributes("aria-label")).toBe("组合市值说明");
    const tip = await hoverTipText(trigger);
    expect(tip).toContain("历史市值");
    expect(tip).toContain("同期历史汇率");
  });
});
