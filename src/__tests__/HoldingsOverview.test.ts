import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { NDataTable } from "naive-ui";
import { nextTick } from "vue";
import { useReferenceStore } from "@/stores/reference";
import { applyLocale } from "@ledger/i18n";
import HoldingsOverview from "@/investment/HoldingsOverview.vue";
import {
  INSTRUMENT_SYNC_PROGRESS_EVENT,
  resetInstrumentInfoSyncForTest,
} from "@/investment/useInstrumentInfoSync";
import { captureListenHandlers } from "@ledger/test-support/listen-mock";
import { componentVm } from "@ledger/test-support/component-vm";
import { formatAmount, formatPrice } from "@ledger/money";
import { probeColor } from "@ledger/test-support/dom";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import { hoverTipText } from "@ledger/test-support/tooltip";
import { pnlSemanticColor } from "@ledger/theme/semantic-colors";
import { useAppStore } from "@/stores/app";
import {
  makeAccount,
  makeHolding,
  makeInstrument,
  makeMwrSummary,
  mockHoldings,
  mockInstruments,
} from "./factories";
import { refCurrencies } from "@ledger/test-support/reference-stubs";
import type { Account, Holding, Instrument } from "@ledger/types";

const cny = refCurrencies[0];
import { firePricesChanged, resetPricesChangedHandler } from "./prices-changed-mock";

vi.mock("@/investment/usePricesChanged", async () => {
  const { capturePricesChangedHandler } = await import("./prices-changed-mock");
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  };
});

const BASE_DEFAULTS = {
  list_holdings: mockHoldings,
  list_instruments: { items: mockInstruments, total: mockInstruments.length },
  cumulative_pnl_native_total: { total_cents: 48000, native_currency: "CNY" },
  money_weighted_return_summary: makeMwrSummary(),
  sync_instrument_info: { synced: 2, skipped: 0, message: "已同步 2 只，跳过 0 只" },
};

beforeEach(async () => {
  resetPricesChangedHandler();
  wireInvokeSeam({ defaults: BASE_DEFAULTS });
  const store = useReferenceStore();
  await store.refresh();
});

let wrapper: ReturnType<typeof mount> | undefined;

async function cellText(colKey: string): Promise<string[]> {
  await nextTick();
  return wrapper!.findAll(`td[data-col-key="${colKey}"]`).map((c) => c.text());
}

describe("HoldingsOverview 当前持仓概览卡（issue #110）", () => {
  it("渲染总市值、持仓收益合计与累计收益（折本位币单值，排除无行情行并计数）", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(wrapper.text()).toContain("当前持仓");
    expect(wrapper.text()).toContain("总市值");
    expect(wrapper.text()).toContain(formatAmount(150000, cny));
    expect(wrapper.find('[data-testid="total-unrealized-pnl-value"]').text()).toBe(
      formatAmount(30000, cny),
    );
    expect(wrapper.find('[data-testid="total-unrealized-pnl-missing-price"]').text()).toBe(
      "1 只持仓因缺现价未计入。",
    );
    expect(wrapper.text()).not.toContain("未实现盈亏");
  });

  it("渲染持仓明细表列：标的/数量/成本/现价/市值/持仓收益", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const headers = wrapper.findAll("th").map((th) => th.text());
    for (const h of [
      "标的",
      "名称",
      "账户",
      "数量",
      "成本",
      "现价",
      "净值日期",
      "市值",
      "持仓收益",
    ]) {
      expect(headers).toContain(h);
    }
    expect(await cellText("symbol")).toEqual(["000001", "600000"]);
    expect(await cellText("quantity")).toEqual(["10", "100"]);
    expect(await cellText("cost_basis")).toEqual([
      formatAmount(8000, cny),
      formatAmount(120000, cny),
    ]);
    expect(await cellText("latest_price")).toEqual(["-同步", formatPrice(150000, cny)]);
    expect(await cellText("market_value")).toEqual(["-", formatAmount(150000, cny)]);
    expect(await cellText("unrealized_pnl")).toEqual(["-", formatAmount(30000, cny)]);
  });

  it("合计区新增「累计收益」卡：折本位币单值透传（issue #1077/#1797 接线负向条目，ADR-0087）", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const card = wrapper.find('[data-testid="total-cumulative-pnl"]');
    expect(card.exists()).toBe(true);
    expect(card.text()).toBe(`累计收益${formatAmount(48000, cny)}`);
  });

  it("合计三卡：盈亏两卡按符号着盈亏涨跌色、市值卡不着色，三卡数值等宽数字", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { cumulative_pnl_native_total: { total_cents: -900, native_currency: "CNY" } },
    });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const theme = useAppStore().theme;
    const inlineColors = (testId: string) =>
      wrapper!
        .findAll(`[data-testid="${testId}-value"] span[style]`)
        .map((s) => (s.element as HTMLElement).style.color);
    expect(inlineColors("total-market-value")).toEqual([]);
    expect(inlineColors("total-unrealized-pnl")).toEqual([
      probeColor(pnlSemanticColor(30000, theme)),
    ]);
    expect(inlineColors("total-cumulative-pnl")).toEqual([
      probeColor(pnlSemanticColor(-900, theme)),
    ]);
    expect(wrapper.findAll(".n-statistic-value")).toHaveLength(3);
    for (const id of ["total-market-value", "total-unrealized-pnl", "total-cumulative-pnl"]) {
      const valueBox = wrapper.find(`[data-testid="${id}"] .n-statistic-value`)
        .element as HTMLElement;
      expect(valueBox.style.fontVariantNumeric, id).toBe("tabular-nums");
    }
  });

  it("多币种持仓折本位币单值：合计不再分组拼接，盈亏卡按单值符号取一处涨跌色（issue #1797）", async () => {
    const usd = { code: "USD", name: "美元", symbol: "$", decimal_places: 2 };
    const usdAccount = makeAccount({ id: "acc-usd", name: "美股账户", currency_code: "USD" });
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_currencies: [cny, usd],
        list_accounts: [makeAccount({ id: "acc-a", name: "证券A" }), usdAccount],
        list_holdings: [
          mockHoldings[0]!,
          makeHolding({
            id: "h-usd",
            account_id: "acc-usd",
            instrument_id: "inst-2",
            cost_basis_cents: 4000,
            cost_currency_code: "USD",
            latest_price_cents: 3500,
            latest_price_currency_code: "USD",
            market_value_cents: 3500,
            unrealized_pnl_cents: -500,
            native_market_value_cents: 26000,
            native_unrealized_pnl_cents: -36000,
          }),
        ],
      },
    });
    await useReferenceStore().refresh();
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const theme = useAppStore().theme;
    const market = wrapper.find('[data-testid="total-market-value-value"]');
    expect(market.text()).toBe(formatAmount(176000, cny));
    expect(market.text()).not.toContain(" / ");
    const pnl = wrapper.find('[data-testid="total-unrealized-pnl-value"]');
    expect(pnl.text()).toBe(formatAmount(-6000, cny));
    expect(pnl.findAll("span[style]").map((s) => (s.element as HTMLElement).style.color)).toEqual([
      probeColor(pnlSemanticColor(-6000, theme)),
    ]);
  });

  it("合计三卡同排：桌面档栅格三列、移动档单列（接窗口分级断点，不自立断点）", async () => {
    setFakeMedia({ width: 1200 });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const gridStyle = () =>
      (wrapper!.find(".n-grid").element as HTMLElement).style.gridTemplateColumns;
    expect(gridStyle()).toBe("repeat(3, minmax(0, 1fr))");
    setFakeMedia({ width: 600 });
    await flushPromises();
    expect(gridStyle()).toBe("repeat(1, minmax(0, 1fr))");
  });

  it("三个概念各有口径说明触发器：指针轴悬停出 tooltip，文案与概念对应", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    for (const id of [
      "total-market-value-info",
      "total-unrealized-pnl-info",
      "total-cumulative-pnl-info",
    ]) {
      expect(wrapper.find(`[data-testid="${id}"]`).exists(), id).toBe(true);
    }
    expect(document.body.textContent).not.toContain("累计分红");
    const tipText = await hoverTipText(wrapper.find('[data-testid="total-cumulative-pnl-info"]'));
    expect(tipText).toContain("已实现盈亏");
    expect(tipText).toContain("累计分红");
    expect(tipText).toContain("全账本");
  });

  it("持仓表列头口径说明：成本/现价/市值/持仓收益/收益率各带说明触发器（issue #1369）", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    for (const id of [
      "holdings-cost-info",
      "holdings-price-info",
      "holdings-market-value-info",
      "holdings-unrealized-pnl-info",
      "holdings-mwr-info",
    ]) {
      expect(wrapper.find(`[data-testid="${id}"]`).exists(), id).toBe(true);
    }
    const mwrTip = await hoverTipText(wrapper.find('[data-testid="holdings-mwr-info"]'));
    expect(mwrTip).toContain("两者不可互算");
    expect(mwrTip).toContain("不随搜索或标的筛选收窄");
  });

  it("触控轴：口径说明点按可达（入弹层注册表的气泡），热区外扩到 ≥48px", async () => {
    setFakeMedia({ width: 600, hover: "none", pointer: "coarse" });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const trigger = wrapper.find('[data-testid="total-market-value-info"]');
    expect(trigger.exists()).toBe(true);
    expect((trigger.element as HTMLElement).style.getPropertyValue("--touch-hit-inset")).toBe(
      "-10px -10px",
    );
    expect(document.body.querySelector(".n-popover")).toBeNull();
    await trigger.trigger("click");
    await flushPromises();
    const popover = document.body.querySelector(".n-popover");
    expect(popover).not.toBeNull();
    expect(popover!.textContent).toContain("数量 × 最新价格");
    expect(popover!.textContent).toContain("缺现价或缺汇率的持仓不计入");
    expect(popover!.textContent).toContain("随当前搜索与账户过滤收窄");
    wrapper!.unmount();
    wrapper = undefined;
  });

  it("无持仓时显示空态", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_holdings: [], list_instruments: { items: [], total: 0 } },
    });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(wrapper.find(".n-empty").exists()).toBe(true);
    expect(wrapper.text()).toContain("暂无持仓");
  });

  it("现价列展示基金净值 4 位小数（万分之一元刻度，ADR-0038）", async () => {
    const fundHolding = makeHolding({
      id: "h-fund",
      instrument_id: "inst-fund",
      quantity: 1000,
      cost_basis_cents: 123400,
      latest_price_cents: 12345,
      latest_price_currency_code: "CNY",
      latest_nav_date: null,
      market_value_cents: 123450,
      unrealized_pnl_cents: 50,
    });
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_holdings: [fundHolding],
        list_instruments: {
          items: [
            ...mockInstruments,
            makeInstrument({ id: "inst-fund", symbol: "000123", name: "净值保真基金" }),
          ],
          total: mockInstruments.length + 1,
        },
      },
    });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(await cellText("latest_price")).toEqual([formatPrice(12345, cny)]);
    expect(await cellText("market_value")).toEqual([formatAmount(123450, cny)]);
    expect(await cellText("unrealized_pnl")).toEqual([formatAmount(50, cny)]);
  });

  it("数量列按录入粒度（至多 4 位小数）抹平 f64 位噪声，不把长小数抛给用户", async () => {
    const noisyHolding = makeHolding({
      id: "h-noisy",
      instrument_id: "inst-1",
      quantity: 2094.5699999999965,
      cost_basis_cents: 120000,
    });
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_holdings: [noisyHolding] },
    });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(await cellText("quantity")).toEqual(["2094.57"]);
  });

  it("净值日期独立成列：基金行有值、股票行显示 -；现价列恢复单行（#303 形态修订，issue #912）", async () => {
    const fundHolding = makeHolding({
      id: "h-fund",
      instrument_id: "inst-fund",
      latest_price_cents: 33480,
      latest_price_currency_code: "CNY",
      latest_nav_date: "2026-01-30",
      market_value_cents: 334800,
      unrealized_pnl_cents: 50,
    });
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_holdings: [mockHoldings[0]!, fundHolding],
        list_instruments: {
          items: [
            ...mockInstruments,
            makeInstrument({ id: "inst-fund", symbol: "110022", name: "易方达消费行业" }),
          ],
          total: mockInstruments.length + 1,
        },
      },
    });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(await cellText("nav_date")).toEqual(["2026-01-30", "-"]);
    expect(await cellText("latest_price")).toEqual([
      formatPrice(33480, cny),
      formatPrice(150000, cny),
    ]);
  });

  it("右上角「同步标的信息」按钮触发同步命令，反馈与标的页一致", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const btn = wrapper.find('[data-testid="sync-instrument-info"]');
    expect(btn.exists()).toBe(true);
    await btn.trigger("click");
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledWith("sync_instrument_info");
    expect(wrapper.text()).toContain("已同步 2 只，跳过 0 只");
  });

  it("同步进行中按钮 loading", async () => {
    let resolveSync!: (v: unknown) => void;
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        sync_instrument_info: () =>
          new Promise((res) => {
            resolveSync = res;
          }),
      },
    });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger("click");
    await nextTick();
    expect(wrapper.find(".n-button--loading").exists()).toBe(true);
    resolveSync({ synced: 2, skipped: 0, message: "已同步 2 只，跳过 0 只" });
    await flushPromises();
    expect(wrapper.find(".n-button--loading").exists()).toBe(false);
  });

  it("价格失效信号触发后重拉一次持仓（现价/市值随最新价刷新，issue #238）", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const callsBefore = mockInvoke.mock.calls.filter(([c]) => c === "list_holdings").length;
    firePricesChanged();
    await flushPromises();
    const callsAfter = mockInvoke.mock.calls.filter(([c]) => c === "list_holdings").length;
    expect(callsAfter).toBe(callsBefore + 1);
  });

  it("同步按钮只发起同步：点击不再直连重拉，重拉由信号驱动（样板移除）", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const callsBefore = mockInvoke.mock.calls.filter(([c]) => c === "list_holdings").length;
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger("click");
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledWith("sync_instrument_info");
    expect(mockInvoke.mock.calls.filter(([c]) => c === "list_holdings").length).toBe(callsBefore);
    firePricesChanged();
    await flushPromises();
    expect(mockInvoke.mock.calls.filter(([c]) => c === "list_holdings").length).toBe(
      callsBefore + 1,
    );
  });

  it("同步进行中在卡顶展示确定进度条，完成后收起、结果消息接棒（issue #897）", async () => {
    let resolveSync!: (v: unknown) => void;
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        sync_instrument_info: () =>
          new Promise((res) => {
            resolveSync = res;
          }),
      },
    });
    resetInstrumentInfoSyncForTest();
    const handlers = captureListenHandlers();
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(wrapper.find('[data-testid="instrument-sync-progress"]').exists()).toBe(false);

    await wrapper.find('[data-testid="sync-instrument-info"]').trigger("click");
    await nextTick();
    handlers.at(-1)!({ event: INSTRUMENT_SYNC_PROGRESS_EVENT, payload: { done: 37, total: 100 } });
    await flushPromises();

    const bar = wrapper.find('[data-testid="instrument-sync-progress"]');
    expect(bar.exists()).toBe(true);
    expect(bar.text()).toContain("同步标的信息 37/100");

    resolveSync({ synced: 100, skipped: 0, message: "已同步 100 只，跳过 0 只" });
    await flushPromises();
    expect(wrapper.find('[data-testid="instrument-sync-progress"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("已同步 100 只，跳过 0 只");
  });

  it("同步失败显示错误消息", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { sync_instrument_info: () => Promise.reject(new Error("网络错误")) },
    });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const callsBefore = mockInvoke.mock.calls.filter(([c]) => c === "list_holdings").length;
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("同步失败：网络错误");
    expect(mockInvoke.mock.calls.filter(([c]) => c === "list_holdings").length).toBe(callsBefore);
  });

  it("同步降级时明示「已降级、本次较慢」（issue #1376 存在性断言，ADR-0087）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        sync_instrument_info: {
          synced: 3,
          skipped: 0,
          message: "已同步 3 只，跳过 0 只",
          bulk_degraded: true,
        },
      },
    });
    resetInstrumentInfoSyncForTest();
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(wrapper.find('[data-testid="instrument-sync-degraded"]').exists()).toBe(false);

    await wrapper.find('[data-testid="sync-instrument-info"]').trigger("click");
    await flushPromises();
    const notice = wrapper.find('[data-testid="instrument-sync-degraded"]');
    expect(notice.exists()).toBe(true);
    expect(notice.text()).toBe("已降级、本次较慢");
  });

  it("正常路径（批量面命中，bulk_degraded:false）不渲染降级标注（issue #1376）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        sync_instrument_info: {
          synced: 3,
          skipped: 0,
          message: "已同步 3 只，跳过 0 只",
          bulk_degraded: false,
        },
      },
    });
    resetInstrumentInfoSyncForTest();
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="instrument-sync-degraded"]').exists()).toBe(false);
  });
});

const FILTER_ACCOUNTS: Account[] = [
  makeAccount({ id: "acc-1", name: "现金", type: "cash" }),
  makeAccount({ id: "acc-a", name: "证券A" }),
  makeAccount({ id: "acc-b", name: "证券B", currency_code: "HKD" }),
];

const FILTER_INSTRUMENTS: Instrument[] = [
  makeInstrument({ id: "inst-1", symbol: "600000", name: "浦发银行", market: "sh" }),
  makeInstrument({ id: "inst-2", symbol: "000001", name: "平安银行", market: "sz" }),
  makeInstrument({
    id: "inst-3",
    symbol: "00700",
    name: "腾讯控股",
    market: "hk",
    currency_code: "HKD",
  }),
  makeInstrument({
    id: "inst-4",
    symbol: "AAPL",
    name: "Apple",
    market: "nasdaq",
    currency_code: "USD",
  }),
];

const FILTER_HOLDINGS: Holding[] = [
  makeHolding({
    id: "h-1",
    account_id: "acc-a",
    instrument_id: "inst-1",
    quantity: 100,
    cost_basis_cents: 120000,
    latest_price_cents: 150000,
    latest_price_currency_code: "CNY",
    market_value_cents: 150000,
    unrealized_pnl_cents: 30000,
    native_market_value_cents: 150000,
    native_unrealized_pnl_cents: 30000,
  }),
  makeHolding({
    id: "h-2",
    account_id: "acc-a",
    instrument_id: "inst-2",
    quantity: 10,
    cost_basis_cents: 8000,
  }),
  makeHolding({
    id: "h-3",
    account_id: "acc-b",
    instrument_id: "inst-3",
    quantity: 1000,
    cost_basis_cents: 30000000,
    latest_price_cents: 200000,
    latest_price_currency_code: "HKD",
    market_value_cents: 20000000,
    unrealized_pnl_cents: -500000,
    native_market_value_cents: 1850000,
    native_unrealized_pnl_cents: -45000,
  }),
  makeHolding({
    id: "h-4",
    account_id: "acc-b",
    instrument_id: "inst-4",
    quantity: 10,
    cost_basis_cents: 400000,
    latest_price_cents: 500000,
    latest_price_currency_code: "USD",
    market_value_cents: 500000,
    unrealized_pnl_cents: 10000,
    native_market_value_cents: 3600000,
    native_unrealized_pnl_cents: 8000,
  }),
];

const FILTER_DEFAULTS = {
  list_holdings: FILTER_HOLDINGS,
  list_instruments: { items: FILTER_INSTRUMENTS, total: FILTER_INSTRUMENTS.length },
  cumulative_pnl_native_total: { total_cents: 26000, native_currency: "CNY" },
  money_weighted_return_summary: makeMwrSummary(),
  sync_instrument_info: { synced: 4, skipped: 0, message: "已同步 4 只，跳过 0 只" },
};

async function typeSearch(wrapper: ReturnType<typeof mount>, text: string) {
  await wrapper.find('[data-testid="holdings-search"] input').setValue(text);
  vi.advanceTimersByTime(300);
  await flushPromises();
}

describe("HoldingsOverview 三维过滤排序（issue #902）", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    resetPricesChangedHandler();
    resetInstrumentInfoSyncForTest();
    wireInvokeSeam({ defaults: FILTER_DEFAULTS, overrides: { list_accounts: FILTER_ACCOUNTS } });
    const store = useReferenceStore();
    await store.refresh();
  });

  afterEach(() => {
    vi.useRealTimers();
    if (wrapper) {
      wrapper.unmount();
      wrapper = undefined;
    }
  });

  it("默认态：全量行按标的代码字母序，合计为全量口径（折本位币单值，缺价行不计入并计数）", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(await cellText("symbol")).toEqual(["000001", "00700", "600000", "AAPL"]);
    expect(wrapper.find('[data-testid="total-market-value-value"]').text()).toBe(
      formatAmount(5600000, cny),
    );
    expect(wrapper.find('[data-testid="total-unrealized-pnl-value"]').text()).toBe(
      formatAmount(-7000, cny),
    );
    expect(wrapper.find('[data-testid="total-market-value-missing-price"]').text()).toBe(
      "1 只持仓因缺现价未计入。",
    );
    expect(await cellText("market_value")).toEqual([
      "-",
      formatAmount(20000000),
      formatAmount(150000, cny),
      formatAmount(500000),
    ]);
  });

  it("搜索按代码/名称/拼音命中，清空后恢复完整列表", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    await typeSearch(wrapper, "00700");
    expect(await cellText("symbol")).toEqual(["00700"]);
    await typeSearch(wrapper, "浦发");
    expect(await cellText("symbol")).toEqual(["600000"]);
    await typeSearch(wrapper, "pfyh");
    expect(await cellText("symbol")).toEqual(["600000"]);
    await typeSearch(wrapper, "");
    expect(await cellText("symbol")).toEqual(["000001", "00700", "600000", "AAPL"]);
  });

  it("账户过滤单选生效，选「全部」（清除）恢复完整列表", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      "update:value",
      "acc-b",
    );
    await nextTick();
    expect(await cellText("symbol")).toEqual(["00700", "AAPL"]);
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      "update:value",
      null,
    );
    await nextTick();
    expect(await cellText("symbol")).toEqual(["000001", "00700", "600000", "AAPL"]);
  });

  it("市值列头升降序：缺价行恒排末尾；第三次点击回默认代码序", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const clickMarketValue = async () => {
      await wrapper!
        .findAll("th")
        .find((th) => th.text() === "市值")!
        .trigger("click");
      await nextTick();
    };
    await clickMarketValue();
    expect(await cellText("symbol")).toEqual(["00700", "AAPL", "600000", "000001"]);
    await clickMarketValue();
    expect(await cellText("symbol")).toEqual(["600000", "AAPL", "00700", "000001"]);
    await clickMarketValue();
    expect(await cellText("symbol")).toEqual(["000001", "00700", "600000", "AAPL"]);
  });

  it("持仓收益列头排序：盈利在前亏损在后，缺价行恒排末尾", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const pnlTh = wrapper.findAll("th").find((th) => th.text() === "持仓收益")!;
    await pnlTh.trigger("click");
    await nextTick();
    expect(await cellText("symbol")).toEqual(["600000", "AAPL", "00700", "000001"]);
    await wrapper
      .findAll("th")
      .find((th) => th.text() === "持仓收益")!
      .trigger("click");
    await nextTick();
    expect(await cellText("symbol")).toEqual(["00700", "AAPL", "600000", "000001"]);
  });

  it("合计随搜索与账户过滤子集更新；排序不改变合计", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      "update:value",
      "acc-b",
    );
    await nextTick();
    const marketTotalText = () => wrapper!.find('[data-testid="total-market-value-value"]').text();
    expect(marketTotalText()).toBe(formatAmount(5450000, cny));
    await wrapper!
      .findAll("th")
      .find((th) => th.text() === "市值")!
      .trigger("click");
    await nextTick();
    await wrapper
      .findAll("th")
      .find((th) => th.text() === "市值")!
      .trigger("click");
    await nextTick();
    expect(marketTotalText()).toBe(formatAmount(5450000, cny));
    await typeSearch(wrapper, "txkg");
    expect(marketTotalText()).toBe(formatAmount(1850000, cny));
  });

  it("「没有持仓」与「筛选条件下无匹配」两种空态可区分", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    await typeSearch(wrapper, "999999");
    expect(wrapper.find('[data-testid="holdings-no-match"]').exists()).toBe(true);
    expect(wrapper.text()).toContain("筛选条件下无匹配持仓");
    expect(wrapper.text()).not.toContain("暂无持仓");
    await typeSearch(wrapper, "");
    expect(wrapper.find('[data-testid="holdings-no-match"]').exists()).toBe(false);
    wireInvokeSeam({
      defaults: FILTER_DEFAULTS,
      overrides: { list_holdings: [], list_instruments: { items: [], total: 0 } },
    });
    wrapper.unmount();
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(wrapper.text()).toContain("暂无持仓");
    expect(wrapper.text()).not.toContain("筛选条件下无匹配持仓");
    expect(wrapper.find('[data-testid="holdings-search"]').exists()).toBe(false);
  });
});

const PAGE_ACCOUNTS: Account[] = [
  makeAccount({ id: "acc-page-1", name: "证券A" }),
  makeAccount({ id: "acc-page-2", name: "证券B", currency_code: "HKD" }),
];

const PAGE_HOLDINGS: Holding[] = Array.from({ length: 25 }, (_, i) => {
  const code = String(i + 1).padStart(3, "0");
  return makeHolding({
    id: `ph-${code}`,
    account_id: i < 22 ? "acc-page-1" : "acc-page-2",
    instrument_id: `pinst-${code}`,
    quantity: 100,
    latest_price_cents: 100000 + i,
    latest_price_currency_code: "CNY",
    market_value_cents: 10000000 + i,
    unrealized_pnl_cents: 1000 + i,
    native_market_value_cents: 10000000 + i,
    native_unrealized_pnl_cents: 1000 + i,
  });
});

const PAGE_INSTRUMENTS: Instrument[] = PAGE_HOLDINGS.map((h, i) =>
  makeInstrument({
    id: h.instrument_id,
    symbol: String(i + 1).padStart(3, "0"),
    name: `标的${String(i + 1).padStart(3, "0")}`,
  }),
);

const PAGE_DEFAULTS = {
  list_holdings: PAGE_HOLDINGS,
  list_instruments: { items: PAGE_INSTRUMENTS, total: PAGE_INSTRUMENTS.length },
  cumulative_pnl_native_total: { total_cents: 0, native_currency: "CNY" },
  money_weighted_return_summary: makeMwrSummary({ by_instrument: [], by_account: [], total: [] }),
  sync_instrument_info: { synced: 25, skipped: 0, message: "已同步 25 只，跳过 0 只" },
};

describe("HoldingsOverview 客户端分页（issue #912）", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    resetPricesChangedHandler();
    resetInstrumentInfoSyncForTest();
    wireInvokeSeam({ defaults: PAGE_DEFAULTS, overrides: { list_accounts: PAGE_ACCOUNTS } });
    const store = useReferenceStore();
    await store.refresh();
  });

  afterEach(async () => {
    vi.useRealTimers();
    if (wrapper) {
      wrapper.unmount();
      wrapper = undefined;
    }
  });

  async function goToPage(page: number) {
    await wrapper!
      .findAll(".n-pagination-item")
      .find((el) => el.text() === String(page))!
      .trigger("click");
    await nextTick();
  }

  it("超一页时分页条出现：首页 20 行，第二页余量 5 行，翻回恢复", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(wrapper.find(".n-pagination").exists()).toBe(true);
    expect(await cellText("symbol")).toHaveLength(20);
    expect((await cellText("symbol"))[0]).toBe("001");
    await goToPage(2);
    expect(await cellText("symbol")).toEqual(["021", "022", "023", "024", "025"]);
    await goToPage(1);
    expect(await cellText("symbol")).toHaveLength(20);
    expect((await cellText("symbol"))[0]).toBe("001");
  });

  it("合计为过滤子集全量口径，翻页不改变合计", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const totalText = () => wrapper!.find('[data-testid="total-market-value"]').text();
    const before = totalText();
    await goToPage(2);
    expect(totalText()).toBe(before);
  });

  it("筛选变化翻页归零：第二页时改账户过滤，回新行集第一页", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    await goToPage(2);
    expect(await cellText("symbol")).toHaveLength(5);
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      "update:value",
      "acc-page-1",
    );
    await nextTick();
    expect(await cellText("symbol")).toHaveLength(20);
    expect((await cellText("symbol"))[0]).toBe("001");
  });

  it("排序变化翻页归零：第二页时点市值列头，回排序后第一页", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    await goToPage(2);
    expect(await cellText("symbol")).toHaveLength(5);
    await wrapper!
      .findAll("th")
      .find((th) => th.text() === "市值")!
      .trigger("click");
    await nextTick();
    expect(await cellText("symbol")).toHaveLength(20);
    expect((await cellText("symbol"))[0]).toBe("025");
  });

  it("搜索变化翻页归零：第二页时搜索命中全量，回新行集第一页", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    await goToPage(2);
    expect(await cellText("symbol")).toHaveLength(5);
    await typeSearch(wrapper, "标的");
    expect(await cellText("symbol")).toHaveLength(20);
    expect((await cellText("symbol"))[0]).toBe("001");
  });

  it("页码随会话保留（issue #1192）：卸载重挂留在离开时的第二页", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    await goToPage(2);
    expect(await cellText("symbol")).toHaveLength(5);
    wrapper.unmount();
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(await cellText("symbol")).toHaveLength(5);
    expect(wrapper.findAll(".n-pagination-item--active").map((el) => el.text())).toEqual(["2"]);
  });

  it("恢复页码超出有效范围时钳制：数据缩到 1 页后不落空页、页码回第 1 页（issue #1192）", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    await goToPage(2);
    expect(await cellText("symbol")).toHaveLength(5);
    wrapper.unmount();
    const shrunk = PAGE_HOLDINGS.slice(0, 15);
    wireInvokeSeam({
      defaults: {
        ...PAGE_DEFAULTS,
        list_holdings: shrunk,
        list_instruments: { items: PAGE_INSTRUMENTS.slice(0, 15), total: 15 },
      },
      overrides: { list_accounts: PAGE_ACCOUNTS },
    });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(await cellText("symbol")).toHaveLength(15);
    expect((await cellText("symbol"))[0]).toBe("001");
    expect(wrapper.find(".n-pagination").exists()).toBe(false);
  });

  it("单页行集不出现分页条（4 行三维过滤夹具回归不变）", async () => {
    wireInvokeSeam({ defaults: FILTER_DEFAULTS, overrides: { list_accounts: FILTER_ACCOUNTS } });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(wrapper.find(".n-pagination").exists()).toBe(false);
    expect(await cellText("symbol")).toEqual(["000001", "00700", "600000", "AAPL"]);
  });

  it("净值日期列名 i18n：en-US 为 NAV Date（用例后还原 zh-CN）", async () => {
    await applyLocale("en-US");
    try {
      wrapper = mount(HoldingsOverview);
      await flushPromises();
      expect(wrapper.findAll("th").map((th) => th.text())).toContain("NAV Date");
    } finally {
      await applyLocale("zh-CN");
    }
  });
});

const GUIDE_HOLDINGS: Holding[] = [
  makeHolding({ id: "gh-manual", instrument_id: "g-inst-manual" }),
  makeHolding({ id: "gh-nav", instrument_id: "g-inst-nav" }),
  makeHolding({ id: "gh-quote", instrument_id: "g-inst-quote" }),
  makeHolding({ id: "gh-none", instrument_id: "g-inst-none" }),
];

const GUIDE_INSTRUMENTS: Instrument[] = [
  makeInstrument({
    id: "g-inst-manual",
    symbol: "a1",
    name: "稳稳地幸福",
    type: "other",
    market: "unknown",
    source: "manual",
    price_channel: "manual",
  }),
  makeInstrument({
    id: "g-inst-nav",
    symbol: "b1",
    name: "场外基金",
    type: "fund",
    market: "unknown",
    source: "manual",
    price_channel: "fund_nav",
  }),
  makeInstrument({ id: "g-inst-quote", symbol: "c1", name: "行情标的", price_channel: "quote" }),
  makeInstrument({
    id: "g-inst-none",
    symbol: "d1",
    name: "无来源标的",
    market: "unknown",
    price_channel: "none",
  }),
];

const GUIDE_DEFAULTS = {
  list_holdings: GUIDE_HOLDINGS,
  list_instruments: { items: GUIDE_INSTRUMENTS, total: GUIDE_INSTRUMENTS.length },
  cumulative_pnl_native_total: { total_cents: 0, native_currency: "CNY" },
  sync_instrument_info: { synced: 1, skipped: 0, message: "已同步 1 只，跳过 0 只" },
};

describe("HoldingsOverview 缺价行可执行引导（issue #1193）", () => {
  beforeEach(async () => {
    resetPricesChangedHandler();
    resetInstrumentInfoSyncForTest();
    wireInvokeSeam({ defaults: GUIDE_DEFAULTS });
    await useReferenceStore().refresh();
  });

  afterEach(() => {
    if (wrapper) {
      wrapper.unmount();
      wrapper = undefined;
    }
  });

  it("行情通道缺价行：引导是既有「同步标的信息」入口，点击即发起同步并出结果回执", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const btn = wrapper.find('[data-testid="missing-price-sync-acc-1-g-inst-quote"]');
    expect(btn.exists()).toBe(true);
    expect(btn.text()).toBe("同步");
    await btn.trigger("click");
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledWith("sync_instrument_info");
    expect(wrapper.text()).toContain("已同步 1 只，跳过 0 只");
  });

  it("净值通道缺价行：引导同样落既有「同步标的信息」入口", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const btn = wrapper.find('[data-testid="missing-price-sync-acc-1-g-inst-nav"]');
    expect(btn.exists()).toBe(true);
    await btn.trigger("click");
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledWith("sync_instrument_info");
    expect(wrapper.text()).toContain("已同步 1 只，跳过 0 只");
  });

  it("手动报价通道缺价行：引导打开既有录价弹窗（不新增第二套入口）", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const btn = wrapper.find('[data-testid="missing-price-quote-acc-1-g-inst-manual"]');
    expect(btn.exists()).toBe(true);
    expect(btn.text()).toBe("录价");
    await btn.trigger("click");
    await nextTick();
    expect(document.body.textContent).toContain("录价 — a1");
  });

  it("每行按自身价格通道分流：四类缺价行现价列呈「引导 + 无引导」对照", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(await cellText("latest_price")).toEqual(["-录价", "-同步", "-同步", "-"]);
    expect(wrapper.find('[data-testid="missing-price-sync-acc-1-g-inst-none"]').exists()).toBe(
      false,
    );
    expect(wrapper.find('[data-testid="missing-price-quote-acc-1-g-inst-none"]').exists()).toBe(
      false,
    );
  });

  it("同一标的跨账户持仓：引导 testid 按账户 + 标的派生，多行互不撞车", async () => {
    wireInvokeSeam({
      defaults: GUIDE_DEFAULTS,
      overrides: {
        list_holdings: [
          makeHolding({ id: "gh-a", account_id: "acc-1", instrument_id: "g-inst-quote" }),
          makeHolding({ id: "gh-b", account_id: "acc-2", instrument_id: "g-inst-quote" }),
        ],
      },
    });
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    expect(wrapper.find('[data-testid="missing-price-sync-acc-1-g-inst-quote"]').exists()).toBe(
      true,
    );
    expect(wrapper.find('[data-testid="missing-price-sync-acc-2-g-inst-quote"]').exists()).toBe(
      true,
    );
    expect(wrapper.find('[data-testid="missing-price-sync-c1"]').exists()).toBe(false);
    expect(wrapper.findAll('[data-testid^="missing-price-sync-"]')).toHaveLength(2);
  });
});

describe("HoldingsOverview 表格列形态（词汇表「表格列形态」约定）", () => {
  it("数值列右对齐 + 等宽数字；名称列唯一弹性（无固定宽、minWidth + ellipsis）", async () => {
    wrapper = mount(HoldingsOverview);
    await flushPromises();
    const columns = wrapper.findComponent(NDataTable).props("columns") as unknown as Array<
      Record<string, unknown>
    >;
    const byKey = (key: string) => columns.find((c) => c.key === key)!;
    for (const key of [
      "quantity",
      "cost_basis",
      "latest_price",
      "nav_date",
      "market_value",
      "unrealized_pnl",
    ]) {
      expect(byKey(key).align, key).toBe("right");
      expect(byKey(key).className, key).toBe("tabular-nums");
    }
    const nameCol = byKey("instrumentName");
    expect(nameCol.width).toBeUndefined();
    expect(nameCol.minWidth).toBeGreaterThan(0);
    expect(nameCol.ellipsis).toBeTruthy();
    for (const key of ["symbol", "accountName", "nav_date"]) {
      expect(typeof byKey(key).width, key).toBe("number");
    }
  });
});
