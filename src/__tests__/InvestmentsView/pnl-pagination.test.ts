import { describe, it, expect, vi, beforeEach } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { nextTick } from "vue";
import { NDataTable } from "naive-ui";
import InvestmentsView from "@/views/InvestmentsView.vue";
import { componentVm } from "@ledger/test-support/component-vm";
import { PNL_PAGE_SIZE, useInvestmentsSessionStore } from "@/investment/investments-session";
import { clickTab } from "@ledger/test-support/dom";
import { mountWithDialog } from "@ledger/test-support/mount";
import { makeInvestmentOverview, makeMwrSummary, makePnlSummary } from "../factories";
import type { AccountPnl, YearPnl } from "@ledger/types";

// 走势图用共享桩组件替代（同 InvestmentsView.test.ts 基座）
vi.mock("vue-chartjs", async () => {
  const { LineChartStub } = await import("../line-chart-stub");
  return { Line: LineChartStub };
});

// 价格失效信号订阅 mock（同 InvestmentsView.test.ts 基座）
vi.mock("@/investment/usePricesChanged", async () => {
  const { capturePricesChangedHandler } = await import("../prices-changed-mock");
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  };
});

// focus 参数读取自路由 query（同 InvestmentsView.test.ts 的可控 mockRoute 先例）
const pushMock = vi.fn();
const mockRoute = { query: {} as Record<string, string> };
vi.mock("vue-router", () => ({
  useRoute: () => mockRoute,
  useRouter: () => ({ push: pushMock }),
}));

const mountView = () => mountWithDialog(InvestmentsView);

// —— 夹具：按年后端升序返回（ORDER BY year），共 10 年 ——
const TEN_YEARS: YearPnl[] = Array.from({ length: 10 }, (_, i) => {
  const year = String(2017 + i);
  return {
    year,
    currency_code: "CNY",
    realized_pnl_cents: (i + 1) * 100,
    dividend_cents: 0,
    realized_gain_cents: (i + 1) * 100,
  };
});

/** 按账户行：后端 ORDER BY account_name，此处即返回序 */
const TEN_ACCOUNTS: AccountPnl[] = Array.from({ length: 10 }, (_, i) => ({
  account_id: `acc-${i + 1}`,
  account_name: `账户${i + 1}`,
  currency_code: "CNY",
  realized_pnl_cents: (i + 1) * 100,
  dividend_cents: 0,
  realized_gain_cents: (i + 1) * 100,
}));

const PNL_DEFAULTS = {
  investment_overview: makeInvestmentOverview(),
  list_instruments: { items: [], total: 0 },
  list_holdings: [],
  instrument_price_staleness: { stale_count: 0, threshold_days: 3 },
  cumulative_pnl_summary: [{ currency_code: "CNY", cumulative_pnl_cents: 0 }],
  portfolio_value_trend: { currency_code: "CNY", points: [] },
  instrument_price_trend: { instrument_id: "inst-1", points: [] },
  realized_pnl_summary: makePnlSummary({ by_year: TEN_YEARS, by_account: TEN_ACCOUNTS }),
  money_weighted_return_summary: makeMwrSummary(),
  list_investment_transactions: { items: [], total: 0 },
};

beforeEach(async () => {
  mockRoute.query = {};
  await wireInvokeSeam({ defaults: PNL_DEFAULTS, refreshReferenceStores: true }).ready;
});

// —— 视图装配多表格：按首列 key 定位目标表（year / account_name / scope 三表互异）——

type NDataTableWrapper = VueWrapper<InstanceType<typeof NDataTable>>;

function tableByFirstKey(wrapper: VueWrapper, key: string): NDataTableWrapper {
  const table = wrapper
    .findAllComponents(NDataTable)
    .find(
      (t) => ((t.props("columns") as Array<{ key?: string | number }>)[0]?.key ?? null) === key,
    );
  expect(table, `应存在首列 key=${key} 的表格`).toBeTruthy();
  return table as NDataTableWrapper;
}

interface TablePagination {
  page: number;
  pageSize: number;
  onChange: (page: number) => void;
}

function paginationOf(table: NDataTableWrapper): TablePagination {
  return table.props("pagination") as TablePagination;
}

function cellTexts(table: NDataTableWrapper, key: string): string[] {
  return table.findAll(`td[data-col-key="${key}"]`).map((c) => c.text());
}

async function openPnl(wrapper: VueWrapper) {
  await clickTab(wrapper, "盈亏");
  await flushPromises();
}

describe("盈亏页两表客户端切片分页（issue #1795）", () => {
  it("按年表：第 1 页为最新 8 年（降序），翻页见更早年份", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openPnl(wrapper);
    const table = tableByFirstKey(wrapper, "year");
    const p = paginationOf(table);
    expect(p.pageSize).toBe(PNL_PAGE_SIZE);
    expect(p.page).toBe(1);
    // 降序：后端升序行集在前端倒转，第 1 页 = 最近年份
    expect(cellTexts(table, "year")).toEqual([
      "2026",
      "2025",
      "2024",
      "2023",
      "2022",
      "2021",
      "2020",
      "2019",
    ]);
    // 翻页条在场（>8 行）
    expect(table.find(".n-pagination").exists()).toBe(true);

    p.onChange(2);
    await flushPromises();
    expect(paginationOf(table).page).toBe(2);
    expect(cellTexts(table, "year")).toEqual(["2018", "2017"]);
  });

  it("按账户表：同款分页、顺序保持后端返回序；MWR 表不分页", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openPnl(wrapper);
    const table = tableByFirstKey(wrapper, "account_name");
    const p = paginationOf(table);
    expect(p.pageSize).toBe(PNL_PAGE_SIZE);
    // 保持后端 ORDER BY account_name 返回序，不引入新排序口径
    expect(cellTexts(table, "account_name").slice(0, 2)).toEqual(["账户1", "账户2"]);
    p.onChange(2);
    await flushPromises();
    expect(paginationOf(table).page).toBe(2);
    expect(cellTexts(table, "account_name")).toEqual(["账户9", "账户10"]);

    const mwr = tableByFirstKey(wrapper, "scope");
    expect(mwr.props("pagination")).toBeFalsy();
  });

  it("≤ 8 行单页：收起翻页条（paginate-single-page=false），行集全量直显", async () => {
    await wireInvokeSeam({
      defaults: {
        ...PNL_DEFAULTS,
        realized_pnl_summary: makePnlSummary({
          by_year: TEN_YEARS.slice(0, 3),
          by_account: TEN_ACCOUNTS.slice(0, 2),
        }),
      },
      refreshReferenceStores: true,
    }).ready;
    const wrapper = mountView();
    await flushPromises();
    await openPnl(wrapper);
    const table = tableByFirstKey(wrapper, "year");
    expect(table.props("paginateSinglePage")).toBe(false);
    expect(table.find(".n-pagination").exists()).toBe(false);
    // 全量直显仍为降序（最新年在前）
    expect(cellTexts(table, "year")).toEqual(["2019", "2018", "2017"]);
  });

  it("页码会话内保留：翻到第 2 页切走页签再回来仍是第 2 页", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openPnl(wrapper);
    paginationOf(tableByFirstKey(wrapper, "year")).onChange(2);
    await flushPromises();
    await clickTab(wrapper, "持仓");
    await flushPromises();
    await clickTab(wrapper, "盈亏");
    await flushPromises();
    expect(paginationOf(tableByFirstKey(wrapper, "year")).page).toBe(2);
  });

  it("筛选变化归零（渲染面）：账户筛选实际变化后两表回到第 1 页", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openPnl(wrapper);
    paginationOf(tableByFirstKey(wrapper, "year")).onChange(2);
    paginationOf(tableByFirstKey(wrapper, "account_name")).onChange(2);
    await flushPromises();
    expect(paginationOf(tableByFirstKey(wrapper, "year")).page).toBe(2);
    expect(paginationOf(tableByFirstKey(wrapper, "account_name")).page).toBe(2);
    // 账户筛选（面板首个 PinyinSelect）实际变化：数据重拉 + 两表页码归零（MoneyWeightedReturn
    // 测试同款驱动）。行集不缩水（桩固定 10 行）也回第 1 页——归零只能来自归零接线，
    // 删除 useRealizedPnl 的 watch 即停留第 2 页变红。
    const select = wrapper.findComponent({ name: "PinyinSelect" });
    componentVm(select).$emit("update:value", "acc-1");
    await flushPromises();
    expect(paginationOf(tableByFirstKey(wrapper, "year")).page).toBe(1);
    expect(paginationOf(tableByFirstKey(wrapper, "account_name")).page).toBe(1);
  });

  it("恢复页码越界：回落到有效页并写回保留态（回退不归零）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openPnl(wrapper);
    // 10 行 × 8 行/页 = 2 页有效；越界页码 99 回落到 2 并写回 store
    const session = useInvestmentsSessionStore();
    session.setPnlYearPage(99);
    await nextTick();
    await flushPromises();
    expect(paginationOf(tableByFirstKey(wrapper, "year")).page).toBe(2);
    expect(session.pnlYearPage).toBe(2);
  });
});
