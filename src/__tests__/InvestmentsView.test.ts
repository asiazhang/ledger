import { describe, it, expect, vi, beforeEach } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { flushPromises } from "@vue/test-utils";
import { defineComponent, h, nextTick } from "vue";
import InvestmentsView from "@/views/InvestmentsView.vue";
import RealizedPnlPanel from "@/investment/RealizedPnlPanel.vue";
import { t } from "@ledger/i18n";
import { formatAmount } from "@ledger/money";
import { clickTab, findTab, probeColor } from "@ledger/test-support/dom";
import { componentVm } from "@ledger/test-support/component-vm";
import { mountWithDialog } from "@ledger/test-support/mount";
import { refCurrencies } from "@ledger/test-support/reference-stubs";
import { useAppStore } from "@/stores/app";
import { TREND_PRESET_DEFAULT, useInvestmentsSessionStore } from "@/investment/investments-session";
import { useWindowGuard } from "@/composables/useWindowGuard";
import { createOverlayToken, resetOverlays } from "@ledger/ui-kit/overlayRegistry";
import { clearViewResets, fireViewReset } from "@/composables/viewResetRegistry";
import { kindSemanticColor, pnlSemanticColor } from "@ledger/theme/semantic-colors";
import { makeInvestmentOverview, makeMwrSummary, makePnlSummary, mockHoldings } from "./factories";
import { firePricesChanged, resetPricesChangedHandler } from "./prices-changed-mock";
import type { Instrument } from "@ledger/types";

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

const pushMock = vi.fn();
const mockRoute = { query: {} as Record<string, string> };
vi.mock("vue-router", () => ({
  useRoute: () => mockRoute,
  useRouter: () => ({ push: pushMock }),
}));

const mountView = () => mountWithDialog(InvestmentsView);

const mockInstruments: Instrument[] = [
  {
    id: "inst-1",
    symbol: "600000",
    type: "stock",
    name: "浦发银行",
    currency_code: "CNY",
    market: "sh",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    source: "eastmoney",
    price_cents: null,
    invested: false,
    price_channel: "quote",
  },
  {
    id: "inst-2",
    symbol: "000001",
    type: "stock",
    name: "平安银行",
    currency_code: "CNY",
    market: "sz",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    source: "eastmoney",
    price_cents: null,
    invested: false,
    price_channel: "quote",
  },
  {
    id: "inst-3",
    symbol: "00700",
    type: "stock",
    name: "腾讯控股",
    currency_code: "HKD",
    market: "hk",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    source: "eastmoney",
    price_cents: null,
    invested: false,
    price_channel: "quote",
  },
];

const INVESTMENT_DEFAULTS = {
  investment_overview: makeInvestmentOverview(),
  list_instruments: { items: mockInstruments, total: mockInstruments.length },
  list_holdings: [],
  instrument_price_staleness: { stale_count: 0, threshold_days: 3 },
  cumulative_pnl_native_total: { total_cents: 45000, native_currency: "CNY" },
  portfolio_value_trend: { currency_code: "CNY", points: [] },
  realized_pnl_summary: { by_year: [], by_account: [] },
  money_weighted_return_summary: makeMwrSummary({ by_instrument: [], by_account: [], total: [] }),
  list_investment_transactions: { items: [], total: 0 },
};

beforeEach(async () => {
  resetOverlays();
  clearViewResets();
  resetPricesChangedHandler();
  await wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, refreshReferenceStores: true }).ready;
});

describe("InvestmentsView 标的 tab", () => {
  it.each(["概览", "盈亏", "持仓", "明细", "标的"])("%s tab 存在", async (tab) => {
    const wrapper = mountView();
    await nextTick();
    expect(findTab(wrapper, tab, { exact: true }), `页签「${tab}」应存在`).toBeTruthy();
  });

  it.each(["搜索代码或名称", "全部市场"])("标的 tab 显示「%s」", async (text) => {
    const wrapper = mountView();
    await nextTick();
    await clickTab(wrapper, "标的");
    expect(wrapper.html()).toContain(text);
  });

  it("标的 tab 分页请求携带 page/page_size", async () => {
    const wrapper = mountView();
    await nextTick();
    await clickTab(wrapper, "标的");
    expect(lastInvokeArgs("list_instruments").filter).toMatchObject({ page: 1, page_size: 50 });
  });

  it("标的列表不再有「走势」入口列（issue #1907：单标的查看随走势页签退役）", async () => {
    const wrapper = mountView();
    await nextTick();
    await clickTab(wrapper, "标的");
    await flushPromises();
    expect(wrapper.find('[data-testid="view-trend-600000"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("走势");
  });
});

describe("InvestmentsView 持仓页签（issue #901）", () => {
  const cny = refCurrencies[0];

  it("页签点击经 store 意图入口落位（NTabs 受控回传，单一路径）", async () => {
    const wrapper = mountView();
    await flushPromises();
    expect(useInvestmentsSessionStore().activeTab).toBe("overview");
    await clickTab(wrapper, "持仓");
    expect(useInvestmentsSessionStore().activeTab).toBe("holdings");
    expect(wrapper.findAll(".n-tabs-tab--active").map((el) => el.text())).toEqual(["持仓"]);
  });

  it("页签顺序为概览/盈亏/持仓/明细/标的/走势，默认选中概览（spec #1532 / issue #1536）", async () => {
    const wrapper = mountView();
    await flushPromises();
    expect(wrapper.findAll(".n-tabs-tab").map((el) => el.text())).toEqual([
      "概览",
      "盈亏",
      "持仓",
      "明细",
      "标的",
    ]);
    expect(wrapper.findAll(".n-tabs-tab--active").map((el) => el.text())).toEqual(["概览"]);
  });

  it("持仓页签完整呈现：折本位币单值合计（含累计收益）+ 持仓明细表 + 同步按钮在位", async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } });
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "持仓");
    expect(wrapper.text()).toContain("当前持仓");
    expect(wrapper.text()).toContain("总市值");
    expect(wrapper.text()).toContain(formatAmount(150000, cny));
    expect(wrapper.find('[data-testid="total-unrealized-pnl-value"]').text()).toBe(
      formatAmount(30000, cny),
    );
    expect(wrapper.find('[data-testid="total-cumulative-pnl-value"]').text()).toBe(
      formatAmount(45000, cny),
    );
    expect(wrapper.text()).not.toContain("未实现盈亏");
    expect(wrapper.text()).toContain("600000");
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(true);
  });

  it("持仓行标的/代码列可点击：跳投资页明细页签 ?tab=detail&account=&instrument=（#1784）", async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } });
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "持仓");
    const links = wrapper.findAll("button.instrument-link");
    const link = links.find((l) => l.text().includes("600000"));
    expect(link, "持仓行标的代码应渲染为可点击链接（600000）").toBeDefined();
    await link!.trigger("click");
    expect(pushMock).toHaveBeenCalledWith({
      name: "investments",
      query: { tab: "detail", account: mockHoldings[0]!.account_id, instrument: "inst-1" },
    });
  });

  it("盈亏页签不再渲染持仓卡（收窄为纯已实现盈亏视图），访问持仓页签后返回亦不残留", async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } });
    const wrapper = mountView();
    await flushPromises();
    expect(wrapper.text()).not.toContain("当前持仓");
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("按年度汇总");
    expect(wrapper.text()).toContain("按账户汇总");
    expect(wrapper.text()).not.toContain("已实现盈亏概览");
    expect(wrapper.text()).not.toContain("按标的汇总");
    await clickTab(wrapper, "持仓");
    await clickTab(wrapper, "盈亏");
    expect(wrapper.text()).not.toContain("当前持仓");
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(false);
  });

  it("盈亏页两表的已实现收益主值着盈亏涨跌色，副行拆出两腿（ADR-0129）", async () => {
    wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: {
        realized_pnl_summary: makePnlSummary({
          by_year: [
            {
              year: "2026",
              native_currency: "CNY",
              realized_pnl_cents: 30000,
              dividend_cents: 0,
              realized_gain_cents: 30000,
            },
            {
              year: "2025",
              native_currency: "CNY",
              realized_pnl_cents: -12345,
              dividend_cents: 40000,
              realized_gain_cents: 27655,
            },
          ],
          by_account: [
            {
              account_id: "acc-1",
              account_name: "证券账户A",
              native_currency: "CNY",
              realized_pnl_cents: -12345,
              dividend_cents: 40000,
              realized_gain_cents: 27655,
            },
          ],
        }),
      },
    });
    const wrapper = mountView();
    await flushPromises();
    const theme = useAppStore().theme;
    const mainColors = wrapper
      .findAll('td[data-col-key="realized_gain_cents"] > div > span')
      .map((s) => (s.element as HTMLElement).style.color);
    expect(mainColors).toEqual([
      probeColor(pnlSemanticColor(30000, theme)),
      probeColor(pnlSemanticColor(27655, theme)),
      probeColor(pnlSemanticColor(27655, theme)),
    ]);
    const legColors = wrapper
      .findAll('td[data-col-key="realized_gain_cents"] > div > div > span')
      .map((s) => (s.element as HTMLElement).style.color);
    expect(legColors).toEqual([
      probeColor(pnlSemanticColor(30000, theme)),
      probeColor(kindSemanticColor("dividend", theme)),
      probeColor(pnlSemanticColor(-12345, theme)),
      probeColor(kindSemanticColor("dividend", theme)),
      probeColor(pnlSemanticColor(-12345, theme)),
      probeColor(kindSemanticColor("dividend", theme)),
    ]);
    const cells = wrapper.findAll('td[data-col-key="realized_gain_cents"]');
    expect(cells.map((c) => c.text())).toEqual([
      `${formatAmount(30000, cny)}已实现盈亏 ${formatAmount(30000, cny)} · 现金分红 ${formatAmount(0, cny)}`,
      `${formatAmount(27655, cny)}已实现盈亏 ${formatAmount(-12345, cny)} · 现金分红 ${formatAmount(40000, cny)}`,
      `${formatAmount(27655, cny)}已实现盈亏 ${formatAmount(-12345, cny)} · 现金分红 ${formatAmount(40000, cny)}`,
    ]);
  });

  it("缺料行格内显式标注「无法计算」，不出两腿数字（#1845，删除渲染接线即红）", async () => {
    wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: {
        realized_pnl_summary: makePnlSummary({
          by_year: [
            {
              year: "2025",
              native_currency: "CNY",
              realized_pnl_cents: null,
              dividend_cents: null,
              realized_gain_cents: null,
            },
          ],
          by_account: [],
        }),
      },
    });
    const wrapper = mountView();
    await flushPromises();

    const cells = wrapper.findAll('td[data-col-key="realized_gain_cents"]');
    expect(cells.length).toBeGreaterThan(0);
    for (const cell of cells) {
      expect(cell.text()).toBe(t("investments.pnl.notComputable"));
      expect(cell.find("div").exists()).toBe(false);
    }
    expect(wrapper.text()).not.toContain(formatAmount(-12345, cny));
  });

  it("表头口径说明句上屏（#1845）：按实现时点周汇率、就近周折本位币", async () => {
    const wrapper = mountView();
    await flushPromises();

    const note = wrapper.find('[data-testid="pnl-calibration-note"]');
    expect(note.exists()).toBe(true);
    expect(note.text()).toBe(t("investments.pnl.calibrationNote"));
  });

  it("价格失效信号触发持仓重查：翻新后的市值合计上屏（自动刷新贯通取数与渲染）", async () => {
    const repriced = [
      {
        ...mockHoldings[0],
        latest_price_cents: 300000,
        market_value_cents: 300000,
        unrealized_pnl_cents: 180000,
        native_market_value_cents: 300000,
        native_unrealized_pnl_cents: 180000,
      },
      mockHoldings[1],
    ];
    let holdingsCalls = 0;
    wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: { list_holdings: () => (holdingsCalls++ === 0 ? mockHoldings : repriced) },
    });
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "持仓");
    expect(wrapper.text()).toContain(formatAmount(150000, cny));
    const callsBefore = mockInvoke.mock.calls.filter(([c]) => c === "list_holdings").length;
    firePricesChanged();
    await flushPromises();
    expect(mockInvoke.mock.calls.filter(([c]) => c === "list_holdings").length).toBe(
      callsBefore + 1,
    );
    expect(wrapper.text()).toContain(formatAmount(300000, cny));
    expect(wrapper.text()).not.toContain(formatAmount(150000, cny));
  });

  it("页签选中会话内保留（issue #1192）：卸载重挂回离开时的持仓页签", async () => {
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "持仓");
    expect(wrapper.findAll(".n-tabs-tab--active").map((el) => el.text())).toEqual(["持仓"]);
    wrapper.unmount();
    const remounted = mountView();
    await flushPromises();
    expect(remounted.findAll(".n-tabs-tab--active").map((el) => el.text())).toEqual(["持仓"]);
  });
});

describe("InvestmentsView 盈亏页签预取保留（display-directive 'show' + key）", () => {
  const pnlFetchCount = () =>
    mockInvoke.mock.calls.filter(([cmd]) => cmd === "realized_pnl_summary").length;

  it("装配即预取盈亏汇总，概览→盈亏切换不重挂面板、不重拉数据", async () => {
    const wrapper = mountView();
    await flushPromises();
    expect(pnlFetchCount()).toBe(1);
    const pnlPanelUid = wrapper.findComponent(RealizedPnlPanel).vm.$.uid;

    await clickTab(wrapper, "盈亏");
    await flushPromises();
    expect(pnlFetchCount()).toBe(1);
    expect(wrapper.findComponent(RealizedPnlPanel).vm.$.uid).toBe(pnlPanelUid);
  });
});

describe("InvestmentsView 价格过期提示（issue #1190）", () => {
  it("无过期：不提示，且不发任何同步请求（检查只读本地库）", async () => {
    const wrapper = mountView();
    await flushPromises();

    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === "instrument_price_staleness"),
    ).toHaveLength(1);
    expect(wrapper.find('[data-testid="price-staleness-alert"]').exists()).toBe(false);
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "sync_instrument_info")).toHaveLength(0);
  });

  it("有过期：提示上屏含过期数量与阈值，按钮一键进入既有同步入口", async () => {
    wireInvokeSeam({
      defaults: {
        ...INVESTMENT_DEFAULTS,
        instrument_price_staleness: { stale_count: 3, threshold_days: 3 },
      },
      refreshReferenceStores: true,
    });
    const wrapper = mountView();
    await flushPromises();

    const alert = wrapper.find('[data-testid="price-staleness-alert"]');
    expect(alert.exists()).toBe(true);
    expect(alert.text()).toContain("3");
    expect(alert.text()).toContain("过期");
    expect(alert.text()).toContain("超过 3 天");
    expect(alert.text()).not.toContain("报表");
    expect(wrapper.find('[data-testid="price-staleness-hint"]').text()).toContain("只刷新现价");
    expect(wrapper.find('[data-testid="price-staleness-hint"]').text()).toContain("后台自动补全");
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(false);
    await wrapper.find('[data-testid="price-staleness-go-sync"]').trigger("click");
    await flushPromises();
    await nextTick();
    expect(useInvestmentsSessionStore().activeTab).toBe("instruments");
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(true);
  });

  it("同步写价后重查：提示随价格变新自动消失（价格失效信号驱动，ADR-0031）", async () => {
    let checks = 0;
    wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: {
        instrument_price_staleness: () =>
          checks++ === 0
            ? { stale_count: 2, threshold_days: 3 }
            : { stale_count: 0, threshold_days: 3 },
      },
      refreshReferenceStores: true,
    });
    const wrapper = mountView();
    await flushPromises();
    expect(wrapper.find('[data-testid="price-staleness-alert"]').exists()).toBe(true);

    firePricesChanged();
    await flushPromises();
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === "instrument_price_staleness"),
    ).toHaveLength(2);
    expect(wrapper.find('[data-testid="price-staleness-alert"]').exists()).toBe(false);
  });
});

describe("InvestmentsView 来源跳转落点（issue #709 / #1907 改址）", () => {
  const focusInstrument: Instrument = {
    ...mockInstruments[0],
    id: "inst-sell",
    symbol: "600519",
    name: "招商银行",
    invested: false,
  };

  function activeTabText(wrapper: ReturnType<typeof mountView>): string {
    return wrapper
      .findAll(".n-tabs-tab--active")
      .map((t) => t.text())
      .join();
  }

  function wireAnchorSeam() {
    return wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: {
        list_instruments: (args?: { filter?: { anchor_id?: string } }) => {
          if (args?.filter?.anchor_id === "inst-sell") {
            return { items: [focusInstrument], total: 101, anchor_page: 3 };
          }
          return { items: mockInstruments, total: mockInstruments.length };
        },
      },
      refreshReferenceStores: true,
    }).ready;
  }

  beforeEach(async () => {
    mockRoute.query = {};
    await wireAnchorSeam();
  });

  it("focus 在场：切到标的页签，以 anchor_id 定位查询且目标行高亮（清仓标的照常可达）", async () => {
    mockRoute.query = { focus: "inst-sell" };
    const wrapper = mountView();
    await flushPromises();

    expect(activeTabText(wrapper)).toContain("标的");
    expect(lastInvokeArgs("list_instruments").filter).toMatchObject({
      anchor_id: "inst-sell",
      page_size: 50,
    });
    expect((lastInvokeArgs("list_instruments").filter as { page?: number }).page).toBeUndefined();
    const focusedRow = wrapper.find("tr.instrument-row-focused");
    expect(focusedRow.exists()).toBe(true);
    expect(focusedRow.text()).toContain("600519");
  });

  it("无 focus：停留默认概览页签，不发起锚点定位查询", async () => {
    const wrapper = mountView();
    await flushPromises();

    expect(activeTabText(wrapper)).toContain("概览");
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "list_instruments")).toBe(false);
  });

  it("读一次语义：消费后 query 再变（页签切换 replace 保留残留 focus 场景）不再消费", async () => {
    mockRoute.query = { focus: "inst-sell" };
    mountView();
    await flushPromises();
    expect(
      mockInvoke.mock.calls.filter(
        ([cmd, args]) =>
          cmd === "list_instruments" &&
          (args as { filter?: { anchor_id?: string } })?.filter?.anchor_id === "inst-sell",
      ).length,
    ).toBe(1);

    mockRoute.query = { tab: "investments", focus: "inst-other" };
    await flushPromises();
    expect(
      mockInvoke.mock.calls.filter(
        ([cmd, args]) =>
          cmd === "list_instruments" &&
          (args as { filter?: { anchor_id?: string } })?.filter?.anchor_id === "inst-sell",
      ).length,
    ).toBe(1);
  });

  it("锚点不命中（无效 id）：回落普通页码、无高亮行（不提供落空的跳转）", async () => {
    mockRoute.query = { focus: "ghost" };
    const wrapper = mountView();
    await flushPromises();

    expect(activeTabText(wrapper)).toContain("标的");
    const call = mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_instruments").at(-1)!;
    expect((call[1] as { filter: { anchor_id: string } }).filter.anchor_id).toBe("ghost");
    expect(wrapper.find("tr.instrument-row-focused").exists()).toBe(false);
  });
});

describe("InvestmentsView 持仓页签会话内保留（issue #1192）", () => {
  async function typeSearch(wrapper: ReturnType<typeof mountView>, text: string) {
    await wrapper.find('[data-testid="holdings-search"] input').setValue(text);
    vi.advanceTimersByTime(300);
    await flushPromises();
  }

  async function clickMarketValueHeader(wrapper: ReturnType<typeof mountView>) {
    await wrapper
      .findAll("th")
      .find((th) => th.text() === "市值")!
      .trigger("click");
    await flushPromises();
  }

  const symbols = (wrapper: ReturnType<typeof mountView>) =>
    wrapper.findAll('td[data-col-key="symbol"]').map((c) => c.text());

  const manyHoldings = Array.from({ length: 25 }, (_, i) => ({
    ...mockHoldings[i % mockHoldings.length]!,
    id: `ph-${i}`,
    instrument_id: mockHoldings[i % mockHoldings.length]!.instrument_id,
  }));

  beforeEach(() => {
    vi.useFakeTimers();
    mockRoute.query = {};
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("切走再切回持仓页签：搜索/排序仍在，行集按恢复的选择现过滤", async () => {
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "持仓");
    await flushPromises();
    await clickMarketValueHeader(wrapper);
    expect(symbols(wrapper)).toEqual(["600000", "000001"]);
    await typeSearch(wrapper, "600");
    expect(symbols(wrapper)).toEqual(["600000"]);
    await clickTab(wrapper, "标的");
    await clickTab(wrapper, "持仓");
    await flushPromises();
    expect(symbols(wrapper)).toEqual(["600000"]);
  });

  it("切走再切回持仓页签：账户过滤与页码仍在（25 行夹具第二页恢复）", async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: manyHoldings } });
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "持仓");
    await flushPromises();
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      "update:value",
      "acc-1",
    );
    await flushPromises();
    await wrapper
      .findAll(".n-pagination-item")
      .find((el) => el.text() === "2")!
      .trigger("click");
    await flushPromises();
    const pageTwoRows = wrapper.findAll('td[data-col-key="symbol"]').length;
    expect(pageTwoRows).toBeLessThan(20);
    await clickTab(wrapper, "标的");
    await clickTab(wrapper, "持仓");
    await flushPromises();
    expect(wrapper.findAll('td[data-col-key="symbol"]').length).toBe(pageTwoRows);
    expect(wrapper.findAll(".n-pagination-item--active").map((el) => el.text())).toEqual(["2"]);
  });

  it("新 pinia 表达冷启动：页签与持仓筛选回默认，且全程零写盘", async () => {
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "持仓");
    await flushPromises();
    await clickMarketValueHeader(wrapper);
    await typeSearch(wrapper, "600");
    expect(symbols(wrapper)).toEqual(["600000"]);
    const keysBefore = Object.keys(localStorage);
    wrapper.unmount();

    setActivePinia(createPinia());
    const coldStore = useInvestmentsSessionStore();
    expect(coldStore.activeTab).toBe("overview");
    expect(coldStore.holdingsSorter).toBeNull();
    expect(coldStore.holdingsSearch).toBe("");
    expect(coldStore.holdingsPage).toBe(1);
    const second = mountView();
    await flushPromises();
    expect(second.findAll(".n-tabs-tab--active").map((el) => el.text())).toEqual(["概览"]);
    await clickTab(second, "持仓");
    await flushPromises();
    expect(symbols(second)).toEqual(["000001", "600000"]);
    expect(Object.keys(localStorage)).toEqual(keysBefore);
  });

  it("越界页码回落到有效范围并写回保留态：行集回增多页也不凭空跳页（ADR-0094 决策 3）", async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: manyHoldings } });
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "持仓");
    await flushPromises();
    await wrapper
      .findAll(".n-pagination-item")
      .find((el) => el.text() === "2")!
      .trigger("click");
    await flushPromises();
    expect(useInvestmentsSessionStore().holdingsPage).toBe(2);
    await clickTab(wrapper, "标的");
    await clickTab(wrapper, "持仓");
    await flushPromises();
    wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: { list_holdings: manyHoldings.slice(0, 5) },
    });
    await clickTab(wrapper, "标的");
    await clickTab(wrapper, "持仓");
    await flushPromises();
    expect(wrapper.findAll('td[data-col-key="symbol"]')).toHaveLength(5);
    expect(useInvestmentsSessionStore().holdingsPage).toBe(1);
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: manyHoldings } });
    await clickTab(wrapper, "标的");
    await clickTab(wrapper, "持仓");
    await flushPromises();
    expect(useInvestmentsSessionStore().holdingsPage).toBe(1);
    expect(wrapper.findAll(".n-pagination-item--active").map((el) => el.text())).toEqual(["1"]);
  });

  it("防抖窗口内离开页签：在途输入不落地（撤销定时器），回显回到已应用值", async () => {
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "持仓");
    await flushPromises();
    await typeSearch(wrapper, "600");
    expect(symbols(wrapper)).toEqual(["600000"]);
    await wrapper.find('[data-testid="holdings-search"] input').setValue("000001");
    await clickTab(wrapper, "标的");
    vi.advanceTimersByTime(300);
    await flushPromises();
    const store = useInvestmentsSessionStore();
    expect(store.holdingsSearch).toBe("600");
    expect(store.holdingsSearchInput).toBe("600");
    await clickTab(wrapper, "持仓");
    await flushPromises();
    expect(symbols(wrapper)).toEqual(["600000"]);
  });
});

describe("InvestmentsView ESC 复位（issue #1192）", () => {
  function mountGuardHost() {
    const Host = defineComponent({
      setup() {
        useWindowGuard();
        return () => h("div");
      },
    });
    return mountWithDialog(Host);
  }

  function fireEscape() {
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
  }

  const symbols = (wrapper: ReturnType<typeof mountView>) =>
    wrapper.findAll('td[data-col-key="symbol"]').map((c) => c.text());

  it("无弹层 ESC：持仓筛选/排序清零回默认（清除保留态本身）", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } });
      const guard = mountGuardHost();
      await flushPromises();
      const wrapper = mountView();
      await flushPromises();
      await clickTab(wrapper, "持仓");
      await flushPromises();
      await wrapper
        .findAll("th")
        .find((th) => th.text() === "市值")!
        .trigger("click");
      await flushPromises();
      await wrapper.find('[data-testid="holdings-search"] input').setValue("600");
      vi.advanceTimersByTime(300);
      await flushPromises();
      expect(symbols(wrapper)).toEqual(["600000"]);

      fireEscape();
      await flushPromises();
      expect(useInvestmentsSessionStore().holdingsSorter).toBeNull();
      expect(useInvestmentsSessionStore().holdingsSearch).toBe("");
      expect(useInvestmentsSessionStore().holdingsSearchInput).toBe("");
      expect(useInvestmentsSessionStore().holdingsPage).toBe(1);
      expect(wrapper.findAll(".n-tabs-tab--active").map((el) => el.text())).toEqual(["概览"]);
      await clickTab(wrapper, "持仓");
      await flushPromises();
      expect(symbols(wrapper)).toEqual(["000001", "600000"]);
      wrapper.unmount();
      const reentered = mountView();
      await flushPromises();
      await clickTab(reentered, "持仓");
      await flushPromises();
      expect(
        (reentered.find('[data-testid="holdings-search"] input').element as HTMLInputElement).value,
      ).toBe("");
      expect(symbols(reentered)).toEqual(["000001", "600000"]);
      reentered.unmount();
      guard.unmount();
    } finally {
      vi.useRealTimers();
    }
  });

  it("无弹层 ESC：页签回概览、走势区间回默认（复位清除保留态本身；#1907 起走势随概览）", async () => {
    const guard = mountGuardHost();
    await flushPromises();
    const wrapper = mountView();
    await flushPromises();
    const radio = wrapper.findAll(".n-radio").find((r) => r.text() === "全部");
    await radio!.find("input").setValue(true);
    await flushPromises();
    expect(useInvestmentsSessionStore().trendPreset).toBe("all");

    fireEscape();
    await flushPromises();
    expect(useInvestmentsSessionStore().activeTab).toBe("overview");
    expect(useInvestmentsSessionStore().trendPreset).toBe(TREND_PRESET_DEFAULT);
    wrapper.unmount();
    guard.unmount();
  });

  it("有弹层时 ESC 不复位：弹层库默认关闭行为接管，保留态不动", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } });
      const guard = mountGuardHost();
      await flushPromises();
      const wrapper = mountView();
      await flushPromises();
      await clickTab(wrapper, "持仓");
      await flushPromises();
      await wrapper.find('[data-testid="holdings-search"] input').setValue("600");
      vi.advanceTimersByTime(300);
      await flushPromises();
      const token = createOverlayToken("modal");
      token.set(true);
      fireEscape();
      await flushPromises();
      expect(symbols(wrapper)).toEqual(["600000"]);
      token.set(false);
      wrapper.unmount();
      guard.unmount();
    } finally {
      vi.useRealTimers();
    }
  });

  it("视图卸载即撤销复位注册：离开投资页后守卫消费不到本视图的保留态", async () => {
    const wrapper = mountView();
    await flushPromises();
    expect(fireViewReset()).toBe(true);
    wrapper.unmount();
    expect(fireViewReset()).toBe(false);
  });
});

describe("InvestmentsView 明细页签基座（issue #1779）", () => {
  it("打开明细页签：以默认筛选与分页请求投资明细（无筛选、第 1 页、默认页大小）", async () => {
    const wrapper = mountView();
    await flushPromises();
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_investment_transactions"),
    ).toHaveLength(0);
    await clickTab(wrapper, "明细");
    await flushPromises();
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_investment_transactions");
    expect(calls).toHaveLength(1);
    expect(lastInvokeArgs("list_investment_transactions").filter).toEqual({
      page: 1,
      page_size: 20,
    });
  });
});
