import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import ReportsView from "@/views/ReportsView.vue";
import { formatAmount } from "@ledger/money";
import type { ReportDateRange } from "@ledger/types";

const pushMock = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push: pushMock }) }));

vi.mock("vue-chartjs", async () => {
  const { BarChartStubWithOptions } = await import("./line-chart-stub");
  return { Bar: BarChartStubWithOptions };
});

const mockRange: ReportDateRange = { min_date: "2020-01-01", max_date: "2027-12-31" };

const mockShares = [
  { category_id: "food", category_name: "餐饮", amount_cents: 5000 },
  { category_id: "transport", category_name: "交通", amount_cents: 3000 },
];

beforeEach(() => {
  wireInvokeSeam({
    defaults: {
      report_date_range: mockRange,
      monthly_summary: [],
      category_shares: mockShares,
      merchant_shares: { rows: [], total_cents: 0 },
    },
  });
});

afterEach(() => {
  pushMock.mockReset();
});

async function mountReports() {
  const wrapper = mount(ReportsView);
  await flushPromises();
  return wrapper;
}

function categoryOptions(wrapper: ReturnType<typeof mount>) {
  return JSON.parse(wrapper.find('.category-chart [data-testid="bar-options"]').text());
}

function percentToggle(wrapper: ReturnType<typeof mount>) {
  return wrapper.find('[data-testid="category-percent-toggle"]');
}

describe("报表分类构成占比触控一击可达（issue #843 两轴对比）", () => {
  it("指针轴：无占比切换按钮（hover tooltip 行为不变），柱尾无占比覆盖", async () => {
    setFakeMedia({ hover: "hover", pointer: "fine" });
    const wrapper = await mountReports();
    expect(percentToggle(wrapper).exists()).toBe(false);
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeUndefined();
  });

  it("触控轴：头部「占比」按钮点按显示——柱尾标注升级为「金额 · 占比%」（分母随层级）", async () => {
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const wrapper = await mountReports();
    const toggle = percentToggle(wrapper);
    expect(toggle.exists()).toBe(true);
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeUndefined();
    expect(toggle.attributes("aria-pressed")).toBe("false");
    await toggle.trigger("click");
    await flushPromises();
    expect(percentToggle(wrapper).attributes("aria-pressed")).toBe("true");
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toEqual([
      `${formatAmount(5000)} · 63%`,
      `${formatAmount(3000)} · 38%`,
    ]);
    await percentToggle(wrapper).trigger("click");
    await flushPromises();
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeUndefined();
  });

  it("占比覆盖不改图数据：柱值与下钻点击载荷不受切换影响", async () => {
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const wrapper = await mountReports();
    await percentToggle(wrapper).trigger("click");
    await flushPromises();
    const data = JSON.parse(wrapper.find('.category-chart [data-testid="bar-data"]').text());
    expect(data.datasets[0].data).toEqual([5000, 3000]);
    expect(mockInvoke).toHaveBeenCalledWith("category_shares", {
      kind: "expense",
      month: null,
      year: null,
      from: expect.any(String),
      to: expect.any(String),
    });
  });

  it("换轴即失效：占比开启后切回指针轴，按钮随轴消失且柱尾覆盖不残留", async () => {
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const wrapper = await mountReports();
    await percentToggle(wrapper).trigger("click");
    await flushPromises();
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeTruthy();

    setFakeMedia({ hover: "hover", pointer: "fine" });
    await flushPromises();
    expect(percentToggle(wrapper).exists()).toBe(false);
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeUndefined();
  });
});
