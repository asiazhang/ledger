import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import { defineComponent, h } from "vue";
import { NButton, NEmpty, NSelect } from "naive-ui";
import QuickTimeRange from "@/components/QuickTimeRange.vue";
import ReportsView from "@/views/ReportsView.vue";
import { categoryColor } from "@ledger/utils/category-chart";
import { useWindowGuard } from "@/composables/useWindowGuard";
import { createOverlayToken, resetOverlays } from "@ledger/ui-kit/overlayRegistry";
import { fireViewReset, clearViewResets } from "@/composables/viewResetRegistry";
import {
  UNCATEGORIZED_ONLY,
  CATEGORY_DRILLDOWN_KINDS,
  MERCHANT_DRILLDOWN_KINDS,
} from "@/transaction/useTransactionFilter";
import { makeCategory, makeFakeSink, resetToastSink } from "./factories";
import { registerToastSink } from "@ledger/loadable";
import { formatAmount } from "@ledger/money";
import type { NullableDateRange } from "@ledger/utils/time-period";
import type { ReportDateRange } from "@ledger/types";

vi.mock("vue-chartjs", async () => {
  const { BarChartStubWithOptions } = await import("./line-chart-stub");
  return { Bar: BarChartStubWithOptions };
});

const pushMock = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push: pushMock }) }));

const Y = 2026;

const mockRange: ReportDateRange = {
  min_date: "2020-03-01",
  max_date: "2027-11-30",
};

const mockCategories = [
  makeCategory({ id: "food", name: "餐饮", sort_order: 0 }),
  makeCategory({ id: "food-snack", name: "零食", parent_id: "food", sort_order: 1 }),
  makeCategory({ id: "transport", name: "交通", sort_order: 2 }),
];

const mockShares = [
  { category_id: "food", category_name: "餐饮", amount_cents: 5000 },
  { category_id: "transport", category_name: "交通", amount_cents: 3000 },
  { category_id: "food-snack", category_name: "零食", amount_cents: 1000 },
  { category_id: "", category_name: "未分类", amount_cents: 800 },
  { category_id: "zero", category_name: "零净额", amount_cents: 0 },
];

const BASE_DEFAULTS = {
  report_date_range: mockRange,
  monthly_summary: [],
  category_shares: [],
  merchant_shares: { rows: [], total_cents: 0 },
};

beforeEach(() => {
  resetToastSink();
  wireInvokeSeam({ defaults: BASE_DEFAULTS });
  pushMock.mockReset();
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0));
});

afterEach(() => {
  vi.useRealTimers();
  resetToastSink();
});

async function mountReports() {
  const wrapper = mount(ReportsView);
  await flushPromises();
  return wrapper;
}

const chip = (wrapper: ReturnType<typeof mount>, label: string) =>
  wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!;

async function clickChip(wrapper: ReturnType<typeof mount>, label: string) {
  await chip(wrapper, label).trigger("click");
  await flushPromises();
}

async function clickBar(wrapper: ReturnType<typeof mount>, index: number) {
  await wrapper.findAll('[data-testid="bar-click"]')[index].trigger("click");
  await flushPromises();
}

function breadcrumbOf(wrapper: ReturnType<typeof mount>) {
  return wrapper.find('[data-testid="category-breadcrumb"]');
}

async function emitPeriod(wrapper: ReturnType<typeof mount>, range: NullableDateRange) {
  wrapper.findComponent(QuickTimeRange).vm.$emit("update:modelValue", range);
  await flushPromises();
}

function monthlyChartProp(prop: "data" | "options", wrapper: ReturnType<typeof mount>) {
  const node = wrapper.findAll(`[data-testid="bar-${prop}"]`)[0];
  return JSON.parse(node.text());
}

function categoryChartProp(prop: "data" | "options", wrapper: ReturnType<typeof mount>) {
  const node = wrapper.find(`.category-chart [data-testid="bar-${prop}"]`);
  return JSON.parse(node.text());
}

describe("ReportsView 期间筛选（issue #411 / ADR-0057）", () => {
  it("进入默认「当年」快照：三卡以当年期间查询，期间边界由组件内化拉取一次", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        monthly_summary: [
          { month: `${Y}-01`, income_cents: 1100, expense_cents: 700, refund_cents: 90 },
        ],
        category_shares: [{ category_id: "food", category_name: "餐饮", amount_cents: 700 }],
        merchant_shares: {
          rows: [
            { merchant_id: "m-1", merchant_name: "超市", amount_cents: 500, transaction_count: 2 },
          ],
          total_cents: 500,
        },
      },
    });
    const wrapper = await mountReports();
    expect(mockInvoke).toHaveBeenCalledWith("monthly_summary", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
    });
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 5,
    });
    expect(mockInvoke).toHaveBeenCalledWith("category_shares", {
      kind: "expense",
      month: null,
      year: null,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
    });
    const rangeCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "report_date_range");
    expect(rangeCalls).toHaveLength(1);
    const data = monthlyChartProp("data", wrapper);
    expect(data.labels).toEqual([`${Y}-01`]);
    expect(data.datasets.map((d: { data: number[] }) => d.data)).toEqual([[1100], [700], [90]]);
    expect(categoryChartProp("data", wrapper).datasets[0].data).toEqual([700]);
    const trs = wrapper.findAll('[data-testid="merchant-table"] tbody tr');
    expect(trs).toHaveLength(1);
    expect(trs[0].find('[data-testid="merchant-amount"]').text()).toBe(formatAmount(500));
  });

  it("年份下拉退役：无年份选择下拉，快捷选择行为唯一时间控件（四枚芯片、无「全部」）", async () => {
    const wrapper = await mountReports();
    expect(wrapper.findComponent(NSelect).exists()).toBe(false);
    expect(wrapper.findComponent(QuickTimeRange).exists()).toBe(true);
    for (const label of ["当月", "当季", "当年", "去年"]) {
      expect(chip(wrapper, label).exists()).toBe(true);
    }
    expect(chip(wrapper, "全部")).toBeUndefined();
  });

  it("点「去年」芯片：三卡以去年期间重算，边界不重复拉取", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        monthly_summary: (args) =>
          args?.year === Y - 1
            ? [{ month: `${Y - 1}-06`, income_cents: 2100, expense_cents: 1700, refund_cents: 190 }]
            : [],
        category_shares: (args) =>
          args?.from === `${Y - 1}-01-01`
            ? [{ category_id: "food", category_name: "餐饮", amount_cents: 1700 }]
            : [],
        merchant_shares: (args) =>
          args?.year === Y - 1
            ? {
                rows: [
                  {
                    merchant_id: "m-2",
                    merchant_name: "咖啡",
                    amount_cents: 1700,
                    transaction_count: 1,
                  },
                ],
                total_cents: 1700,
              }
            : { rows: [], total_cents: 0 },
      },
    });
    const wrapper = await mountReports();
    expect(wrapper.text()).toContain("本期暂无数据");
    mockInvoke.mockClear();
    await clickChip(wrapper, "去年");
    expect(mockInvoke).toHaveBeenCalledWith("monthly_summary", {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    });
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
      topN: 5,
    });
    expect(mockInvoke).toHaveBeenCalledWith("category_shares", {
      kind: "expense",
      month: null,
      year: null,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    });
    const rangeCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "report_date_range");
    expect(rangeCalls).toHaveLength(0);
    const data = monthlyChartProp("data", wrapper);
    expect(data.labels).toEqual([`${Y - 1}-06`]);
    expect(data.datasets.map((d: { data: number[] }) => d.data)).toEqual([[2100], [1700], [190]]);
    expect(categoryChartProp("data", wrapper).datasets[0].data).toEqual([1700]);
    const trs = wrapper.findAll('[data-testid="merchant-table"] tbody tr');
    expect(trs).toHaveLength(1);
    expect(trs[0].find('[data-testid="merchant-amount"]').text()).toBe(formatAmount(1700));
  });

  it("重复点同一段期间的芯片不重复刷新（同值守卫）", async () => {
    const wrapper = await mountReports();
    mockInvoke.mockClear();
    await clickChip(wrapper, "当年");
    const reportCalls = mockInvoke.mock.calls.filter(([cmd]) =>
      ["monthly_summary", "category_shares", "merchant_shares"].includes(cmd),
    );
    expect(reportCalls).toHaveLength(0);
  });

  it("点「当月」芯片：月度收支卡单组柱如实展示（月期间不切日粒度）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        monthly_summary: [
          { month: `${Y}-01`, income_cents: 1000, expense_cents: 500, refund_cents: 100 },
        ],
      },
    });
    const wrapper = await mountReports();
    mockInvoke.mockClear();
    await clickChip(wrapper, "当月");
    expect(mockInvoke).toHaveBeenCalledWith("monthly_summary", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-01-31`,
    });
    const data = monthlyChartProp("data", wrapper);
    expect(data.labels).toEqual([`${Y}-01`]);
    expect(data.datasets.map((d: { data: number[] }) => d.data)).toEqual([[1000], [500], [100]]);
  });

  it("所选期间无流水：三卡显示空态而非旧数据残留", async () => {
    const wrapper = await mountReports();
    const empties = wrapper.findAllComponents(NEmpty);
    expect(empties.length).toBeGreaterThanOrEqual(3);
    expect(wrapper.text()).toContain("本期暂无数据");
    expect(wrapper.text()).toContain("暂无支出数据");
    expect(wrapper.text()).toContain("本期暂无商户消费");
  });

  it("期间选择不持久化：切换期间 localStorage 零写入", async () => {
    const wrapper = await mountReports();
    const keysBefore = Object.keys(localStorage);
    await clickChip(wrapper, "去年");
    expect(Object.keys(localStorage)).toEqual(keysBefore);
  });
});

describe("ReportsView 支出分类构成横向柱状图（issue #378）", () => {
  it("横向柱状图：indexAxis 为 y，图卡正常挂载", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    const options = categoryChartProp("options", wrapper);
    expect(options.indexAxis).toBe("y");
    expect(options.plugins.legend.display).toBe(false);
    expect(wrapper.find('.category-chart [data-testid="bar-data"]').exists()).toBe(true);
  });

  it("图数据形态：一级归并（二级并入根）+ 未分类柱，净额降序，净额 0 不进图", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    const data = categoryChartProp("data", wrapper);
    expect(data.labels).toEqual(["餐饮", "交通", "未分类"]);
    expect(data.datasets[0].data).toEqual([6000, 3000, 800]);
  });

  it("配色：与柱同序按 id 稳定取色，未分类固定灰", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    const data = categoryChartProp("data", wrapper);
    const colors: string[] = data.datasets[0].backgroundColor;
    expect(colors).toHaveLength(3);
    expect(colors[2]).toBe("#909399");
    expect(colors[0]).not.toBe("#909399");
    expect(colors[1]).not.toBe(colors[0]);
  });

  it("同分类跨期间颜色稳定：切期间后同 id 的柱颜色不变", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    const before = categoryChartProp("data", wrapper).datasets[0].backgroundColor;
    await clickChip(wrapper, "去年");
    const after = categoryChartProp("data", wrapper).datasets[0].backgroundColor;
    expect(after).toEqual(before);
  });

  it("全部平铺、卡片内滚动：容器限高滚动、图高随行数增长不截断", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    const scroll = wrapper.find('[data-testid="category-chart-scroll"]');
    expect(scroll.exists()).toBe(true);
    expect(scroll.attributes("style")).toContain("overflow-y: auto");
    const inner = wrapper.find('[data-testid="category-chart-canvas"]');
    expect(inner.attributes("style")).toContain("height: 192px");
  });

  it("汇总层级切换器已退役：不再渲染 NRadioGroup", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    expect(wrapper.findComponent({ name: "NRadioGroup" }).exists()).toBe(false);
  });

  it("localStorage 残留汇总层级键无副作用：原样保留、渲染正常、不写回", async () => {
    localStorage.setItem("view_state:reports_group_level", '"level1"');
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    expect(localStorage.getItem("view_state:reports_group_level")).toBe('"level1"');
    expect(categoryChartProp("data", wrapper).datasets[0].data).toEqual([6000, 3000, 800]);
    localStorage.removeItem("view_state:reports_group_level");
  });
});

describe("ReportsView 分类图内下钻 + 面包屑（issue #379）", () => {
  it("点一级柱图内下钻：行集合 = 直挂行 + 二级子分类行，合计 = 父柱金额（不触发跳转）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    await clickBar(wrapper, 0);
    const data = categoryChartProp("data", wrapper);
    expect(data.labels).toEqual(["餐饮（直挂）", "零食"]);
    expect(data.datasets[0].data).toEqual([5000, 1000]);
    expect(data.datasets[0].data.reduce((a: number, b: number) => a + b, 0)).toBe(6000);
    expect(pushMock).not.toHaveBeenCalled();
  });

  it("下钻态配色沿用同一稳定映射：直挂行同父柱色，二级行同分类色", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    const baseColors: string[] = categoryChartProp("data", wrapper).datasets[0].backgroundColor;
    await clickBar(wrapper, 0);
    const colors: string[] = categoryChartProp("data", wrapper).datasets[0].backgroundColor;
    expect(colors[0]).toBe(baseColors[0]);
    expect(colors[1]).toBe(categoryColor("food-snack"));
  });

  it("面包屑显示当前位置（全部分类 › 分类名），点根返回基础态", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    expect(breadcrumbOf(wrapper).exists()).toBe(false);
    await clickBar(wrapper, 0);
    expect(breadcrumbOf(wrapper).exists()).toBe(true);
    expect(breadcrumbOf(wrapper).text()).toContain("全部分类");
    expect(breadcrumbOf(wrapper).text()).toContain("餐饮");
    await wrapper.find('[data-testid="breadcrumb-root"]').trigger("click");
    await flushPromises();
    expect(breadcrumbOf(wrapper).exists()).toBe(false);
    expect(categoryChartProp("data", wrapper).labels).toEqual(["餐饮", "交通", "未分类"]);
  });

  it("未分类柱不进图内下钻，直达「仅无分类」列表：载荷 = 保留值 + 当年首尾日期 + 收支类型集合（issue #581）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    await clickBar(wrapper, 2);
    expect(pushMock).toHaveBeenCalledTimes(1);
    expect(pushMock).toHaveBeenCalledWith({
      name: "transactions",
      query: {
        category: UNCATEGORIZED_ONLY,
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-12-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    });
    expect(breadcrumbOf(wrapper).exists()).toBe(false);
    expect(categoryChartProp("data", wrapper).labels).toEqual(["餐饮", "交通", "未分类"]);
  });

  it("下钻态点二级子分类行：跳转该分类精确过滤，载荷带当年首尾日期", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    await clickBar(wrapper, 0);
    pushMock.mockClear();
    await clickBar(wrapper, 1);
    expect(pushMock).toHaveBeenCalledWith({
      name: "transactions",
      query: {
        category: "food-snack",
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-12-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    });
  });

  it("下钻态点父直挂行：按父分类精确过滤（载荷 category = 父分类 id）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    await clickBar(wrapper, 0);
    pushMock.mockClear();
    await clickBar(wrapper, 0);
    expect(pushMock).toHaveBeenCalledWith({
      name: "transactions",
      query: {
        category: "food",
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-12-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    });
  });

  it("跳转载荷 = 所选期间首尾日期（#412 期间化）：「去年」芯片后未分类柱带去年年界", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    await clickChip(wrapper, "去年");
    await clickBar(wrapper, 2);
    expect(pushMock).toHaveBeenCalledWith({
      name: "transactions",
      query: {
        category: UNCATEGORIZED_ONLY,
        dateFrom: `${Y - 1}-01-01`,
        dateTo: `${Y - 1}-12-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    });
  });

  it("跳转载荷随月期间（#412）：「当月」芯片后未分类柱带当月月界", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    await clickChip(wrapper, "当月");
    await clickBar(wrapper, 2);
    expect(pushMock).toHaveBeenCalledWith({
      name: "transactions",
      query: {
        category: UNCATEGORIZED_ONLY,
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-01-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    });
  });

  it("跳转载荷随季期间（#412）：「当季」芯片后下钻二级行带当季季界", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    await clickChip(wrapper, "当季");
    await clickBar(wrapper, 0);
    pushMock.mockClear();
    await clickBar(wrapper, 1);
    expect(pushMock).toHaveBeenCalledWith({
      name: "transactions",
      query: {
        category: "food-snack",
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-03-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    });
  });

  it("切换期间复位基础态；下钻态不持久化（localStorage 零写入）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const wrapper = await mountReports();
    const keysBefore = Object.keys(localStorage);
    await clickBar(wrapper, 0);
    expect(Object.keys(localStorage)).toEqual(keysBefore);
    await clickChip(wrapper, "去年");
    expect(breadcrumbOf(wrapper).exists()).toBe(false);
    expect(categoryChartProp("data", wrapper).labels).toEqual(["餐饮", "交通", "未分类"]);
  });

  it("步进器/面板产出的任意月期间同样驱动三卡重算（受控 v-model 桥接）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_categories: mockCategories,
        monthly_summary: (args) =>
          args?.from === "2025-12-01"
            ? [{ month: "2025-12", income_cents: 3000, expense_cents: 2200, refund_cents: 200 }]
            : [],
        category_shares: (args) =>
          args?.from === "2025-12-01"
            ? [{ category_id: "food", category_name: "餐饮", amount_cents: 2200 }]
            : mockShares,
        merchant_shares: (args) =>
          args?.from === "2025-12-01"
            ? {
                rows: [
                  {
                    merchant_id: "m-3",
                    merchant_name: "书店",
                    amount_cents: 2200,
                    transaction_count: 3,
                  },
                ],
                total_cents: 2200,
              }
            : { rows: [], total_cents: 0 },
      },
    });
    const wrapper = await mountReports();
    mockInvoke.mockClear();
    await emitPeriod(wrapper, { from: "2025-12-01", to: "2025-12-31" });
    expect(mockInvoke).toHaveBeenCalledWith("monthly_summary", {
      year: 2025,
      from: "2025-12-01",
      to: "2025-12-31",
    });
    expect(mockInvoke).toHaveBeenCalledWith("category_shares", {
      kind: "expense",
      month: null,
      year: null,
      from: "2025-12-01",
      to: "2025-12-31",
    });
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: 2025,
      from: "2025-12-01",
      to: "2025-12-31",
      topN: 5,
    });
    const data = monthlyChartProp("data", wrapper);
    expect(data.labels).toEqual(["2025-12"]);
    expect(data.datasets.map((d: { data: number[] }) => d.data)).toEqual([[3000], [2200], [200]]);
    expect(categoryChartProp("data", wrapper).datasets[0].data).toEqual([2200]);
    const trs = wrapper.findAll('[data-testid="merchant-table"] tbody tr');
    expect(trs).toHaveLength(1);
    expect(trs[0].find('[data-testid="merchant-amount"]').text()).toBe(formatAmount(2200));
  });
});

describe("ReportsView 会话内保留（issue #427）：同一 pinia 卸载重挂恢复，新 pinia 冷启动", () => {
  it("选期间 + 图内下钻后卸载重挂（同一会话）：期间与下钻面包屑恢复，三卡以恢复期间重拉非缓存数据", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const first = await mountReports();
    await clickChip(first, "去年");
    await clickBar(first, 0);
    expect(breadcrumbOf(first).exists()).toBe(true);
    first.unmount();

    const freshShares = [{ category_id: "food", category_name: "餐饮", amount_cents: 9000 }];
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: freshShares },
    });
    mockInvoke.mockClear();
    const second = await mountReports();

    expect(mockInvoke).toHaveBeenCalledWith("monthly_summary", {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    });
    expect(mockInvoke).toHaveBeenCalledWith("category_shares", {
      kind: "expense",
      month: null,
      year: null,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    });
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
      topN: 5,
    });

    expect(breadcrumbOf(second).exists()).toBe(true);
    expect(breadcrumbOf(second).text()).toContain("餐饮");
    const data = categoryChartProp("data", second);
    expect(data.labels).toEqual(["餐饮（直挂）"]);
    expect(data.datasets[0].data).toEqual([9000]);

    expect(pushMock).not.toHaveBeenCalled();
  });

  it("恢复期间点亮对应芯片：去年快照恢复后「去年」芯片高亮（primary）", async () => {
    wireInvokeSeam({ defaults: BASE_DEFAULTS });
    const first = await mountReports();
    await clickChip(first, "去年");
    first.unmount();

    const second = await mountReports();
    const lastYearChip = chip(second, "去年");
    expect(lastYearChip.props("type")).toBe("primary");
    expect(chip(second, "当年").props("type")).toBe("default");
  });

  it("多次往返（报表 → 交易 → 报表 → 更多 → 报表）：恢复最近一次离开时的样子", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const first = await mountReports();
    await clickChip(first, "当月");
    first.unmount();
    const second = await mountReports();
    expect(mockInvoke).toHaveBeenCalledWith("monthly_summary", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-01-31`,
    });
    await clickChip(second, "去年");
    await clickBar(second, 0);
    second.unmount();

    mockInvoke.mockClear();
    const third = await mountReports();
    expect(mockInvoke).toHaveBeenCalledWith("monthly_summary", {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    });
    expect(breadcrumbOf(third).exists()).toBe(true);
    expect(breadcrumbOf(third).text()).toContain("餐饮");
  });

  it("新 pinia + 重挂表达冷启动：回默认「当年」，下钻回基础态，面包屑不出现", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const first = await mountReports();
    await clickChip(first, "去年");
    await clickBar(first, 0);
    first.unmount();

    setActivePinia(createPinia());
    mockInvoke.mockClear();
    const second = await mountReports();
    expect(mockInvoke).toHaveBeenCalledWith("monthly_summary", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
    });
    expect(breadcrumbOf(second).exists()).toBe(false);
    expect(categoryChartProp("data", second).labels).toEqual(["餐饮", "交通", "未分类"]);
  });

  it("会话内保留零持久化：选择期间与下钻全程 localStorage 零写入", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const first = await mountReports();
    const keysBefore = Object.keys(localStorage);
    await clickChip(first, "去年");
    await clickBar(first, 0);
    first.unmount();
    const second = await mountReports();
    expect(Object.keys(localStorage)).toEqual(keysBefore);
    second.unmount();
  });
});

describe("ReportsView 加载失败治愈（issue #1008）", () => {
  it("整页主实例失败：默认策略弹裸 errorMessage（治愈原 try/finally 无 catch 的静默失败）", async () => {
    const sink = makeFakeSink();
    registerToastSink(sink);
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { monthly_summary: () => Promise.reject(new Error("数据库不可用")) },
    });
    await mountReports();
    expect(sink.error).toHaveBeenCalledWith("数据库不可用");
  });
});

describe("ReportsView 商户排行表格化 + TopN（issue #588 → #618）", () => {
  const mockMerchants = [
    { merchant_id: "m-1", merchant_name: "超市", amount_cents: 5000, transaction_count: 3 },
    { merchant_id: "m-2", merchant_name: "咖啡", amount_cents: 3000, transaction_count: 2 },
    { merchant_id: "m-3", merchant_name: "书店", amount_cents: 1000, transaction_count: 1 },
  ];

  const merchantPayload = (rows = mockMerchants) => ({ rows, total_cents: 15000 });

  const merchantRefs = mockMerchants.map((m) => ({
    id: m.merchant_id,
    name: m.merchant_name,
    is_deleted: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
  }));

  async function clickMerchantName(wrapper: ReturnType<typeof mount>, index = 0) {
    const trs = wrapper.findAll('[data-testid="merchant-table"] tbody tr');
    await trs[index].find('[data-testid="merchant-name"]').trigger("click");
    await flushPromises();
  }

  async function clickTopN(wrapper: ReturnType<typeof mount>, n: number) {
    await wrapper.find(`[data-testid="merchant-topn-${n}"] input`).setValue(true);
    await flushPromises();
  }

  it("表格行序 = 后端返回序：商户名、金额、占比、笔数逐行渲染（口径归纯函数，此处锁视图接线）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_merchants: merchantRefs, merchant_shares: merchantPayload() },
    });
    const wrapper = await mountReports();
    const trs = wrapper.findAll('[data-testid="merchant-table"] tbody tr');
    expect(trs).toHaveLength(3);
    expect(trs[0].find('[data-testid="merchant-name"]').text()).toBe("超市");
    expect(trs[0].find('[data-testid="merchant-amount"]').text()).toBe(formatAmount(5000));
    expect(trs[0].find('[data-testid="merchant-share"]').text()).toBe("33%");
    expect(trs[0].find('[data-testid="merchant-count"]').text()).toBe("3");
  });

  it("默认 Top 5：进入即以 top_n=5 查询", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { merchant_shares: merchantPayload() },
    });
    await mountReports();
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 5,
    });
  });

  it("切 Top 10：仅商户卡以 top_n=10 重拉，其余两卡不受牵连", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { merchant_shares: merchantPayload() },
    });
    const wrapper = await mountReports();
    mockInvoke.mockClear();
    await clickTopN(wrapper, 10);
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 10,
    });
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "monthly_summary")).toHaveLength(0);
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "category_shares")).toHaveLength(0);
  });

  it("TopN 会话内保留：同 pinia 卸载重挂以 Top 10 重拉；冷启动（新 pinia）回默认 5", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { merchant_shares: merchantPayload() },
    });
    const first = await mountReports();
    await clickTopN(first, 10);
    first.unmount();

    mockInvoke.mockClear();
    const second = await mountReports();
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 10,
    });
    second.unmount();

    setActivePinia(createPinia());
    mockInvoke.mockClear();
    await mountReports();
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 5,
    });
  });

  it("TopN 切换零持久化：localStorage 零写入", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { merchant_shares: merchantPayload() },
    });
    const wrapper = await mountReports();
    const keysBefore = Object.keys(localStorage);
    await clickTopN(wrapper, 10);
    expect(Object.keys(localStorage)).toEqual(keysBefore);
  });

  it("TopN 快速连点竞态：最后一次发起胜出，迟到的前发响应丢弃（merchant Loadable 实例）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { merchant_shares: merchantPayload() },
    });
    const wrapper = await mountReports();
    let releaseTop10!: (v: unknown) => void;
    const pendingTop10 = new Promise((resolve) => {
      releaseTop10 = resolve;
    });
    const top5Payload = {
      rows: [
        { merchant_id: "m-9", merchant_name: "快餐", amount_cents: 500, transaction_count: 2 },
      ],
      total_cents: 15000,
    };
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        merchant_shares: (args) =>
          args?.topN === 10 ? pendingTop10 : Promise.resolve(top5Payload),
      },
    });
    await clickTopN(wrapper, 10);
    await clickTopN(wrapper, 5);
    releaseTop10({
      rows: [
        { merchant_id: "m-x", merchant_name: "迟到户", amount_cents: 9, transaction_count: 1 },
      ],
      total_cents: 9,
    });
    await flushPromises();
    const trs = wrapper.findAll('[data-testid="merchant-table"] tbody tr');
    expect(trs).toHaveLength(1);
    expect(trs[0].find('[data-testid="merchant-amount"]').text()).toBe(formatAmount(500));
  });

  it("点商户名跳传交易列表（#589 → #618）：载荷 = 商户 id + 所选期间首尾日期 + 收支类型集合（支出+退款）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_merchants: merchantRefs, merchant_shares: merchantPayload() },
    });
    const wrapper = await mountReports();
    pushMock.mockClear();
    await clickMerchantName(wrapper);
    expect(pushMock).toHaveBeenCalledTimes(1);
    expect(pushMock).toHaveBeenCalledWith({
      name: "transactions",
      query: {
        merchant: "m-1",
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-12-31`,
        kinds: MERCHANT_DRILLDOWN_KINDS,
      },
    });
  });

  it("商户下钻载荷随期间（#589 边界）：选「去年」后点商户名带去年年界", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_merchants: merchantRefs, merchant_shares: merchantPayload() },
    });
    const wrapper = await mountReports();
    await clickChip(wrapper, "去年");
    pushMock.mockClear();
    await clickMerchantName(wrapper);
    expect(pushMock).toHaveBeenCalledWith({
      name: "transactions",
      query: {
        merchant: "m-1",
        dateFrom: `${Y - 1}-01-01`,
        dateTo: `${Y - 1}-12-31`,
        kinds: MERCHANT_DRILLDOWN_KINDS,
      },
    });
  });
});

describe("ReportsView ESC 复位接线（issue #894 / spec #892）：注册 → 守卫消费 → 清除保留态本身", () => {
  beforeEach(() => {
    resetOverlays();
    clearViewResets();
  });

  afterEach(() => {
    clearViewResets();
  });

  function mountGuardHost() {
    const Host = defineComponent({
      setup() {
        useWindowGuard();
        return () => h("div");
      },
    });
    return mount(Host);
  }

  function fireEscape() {
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
  }

  it("无弹层 ESC 触发复位出口：期间回默认「当年」、下钻回基础态、TopN 回默认档，重拉照常；复位后卸载重挂 = 默认", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const guard = mountGuardHost();
    const first = await mountReports();
    await clickChip(first, "去年");
    await clickBar(first, 0);
    await first.find('[data-testid="merchant-topn-10"] input').setValue(true);
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
      topN: 10,
    });

    mockInvoke.mockClear();
    fireEscape();
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledWith("monthly_summary", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
    });
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 5,
    });
    expect(mockInvoke).toHaveBeenCalledWith("category_shares", {
      kind: "expense",
      month: null,
      year: null,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
    });
    expect(breadcrumbOf(first).exists()).toBe(false);
    expect(categoryChartProp("data", first).labels).toEqual(["餐饮", "交通", "未分类"]);
    expect(chip(first, "当年").props("type")).toBe("primary");
    first.unmount();

    const second = await mountReports();
    expect(mockInvoke).toHaveBeenCalledWith("merchant_shares", {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 5,
    });
    expect(breadcrumbOf(second).exists()).toBe(false);
    second.unmount();
    guard.unmount();
  });

  it("仅下钻偏离（期间/TopN 已默认）：ESC 回基础态且不重拉（下钻是纯视图投影，同源数据）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const guard = mountGuardHost();
    const wrapper = await mountReports();
    await clickBar(wrapper, 0);
    expect(breadcrumbOf(wrapper).exists()).toBe(true);
    mockInvoke.mockClear();
    fireEscape();
    await flushPromises();
    expect(breadcrumbOf(wrapper).exists()).toBe(false);
    expect(categoryChartProp("data", wrapper).labels).toEqual(["餐饮", "交通", "未分类"]);
    const reportCalls = mockInvoke.mock.calls.filter(([cmd]) =>
      ["monthly_summary", "category_shares", "merchant_shares"].includes(cmd as string),
    );
    expect(reportCalls).toHaveLength(0);
    wrapper.unmount();
    guard.unmount();
  });

  it("无保留状态（全默认）ESC 幂等：不产生任何重拉", async () => {
    const guard = mountGuardHost();
    const wrapper = await mountReports();
    mockInvoke.mockClear();
    fireEscape();
    await flushPromises();
    expect(mockInvoke).not.toHaveBeenCalled();
    wrapper.unmount();
    guard.unmount();
  });

  it("有弹层时 ESC 不复位：弹层库默认关闭行为接管，保留态不动（一次按键只做一件事）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_categories: mockCategories, category_shares: mockShares },
    });
    const guard = mountGuardHost();
    const wrapper = await mountReports();
    await clickChip(wrapper, "去年");
    await clickBar(wrapper, 0);
    const token = createOverlayToken("modal");
    token.set(true);
    fireEscape();
    await flushPromises();
    expect(breadcrumbOf(wrapper).exists()).toBe(true);
    expect(categoryChartProp("data", wrapper).labels).toEqual(["餐饮（直挂）", "零食"]);
    token.set(false);
    wrapper.unmount();
    guard.unmount();
  });

  it("视图卸载后复位注册自动撤销：守卫消费不再触达本视图（导航离开不滞留注册态）", async () => {
    const guard = mountGuardHost();
    const wrapper = await mountReports();
    expect(fireViewReset()).toBe(true);
    wrapper.unmount();
    expect(fireViewReset()).toBe(false);
    guard.unmount();
  });
});
