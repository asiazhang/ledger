import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { nextTick } from "vue";
import { applyLocale } from "@ledger/i18n";
import { clickTab } from "@ledger/test-support/dom";
import { mountWithDialog } from "@ledger/test-support/mount";
import InvestmentsView from "@/views/InvestmentsView.vue";
import InvestmentForm from "@/investment/InvestmentForm.vue";
import { makeInstrument, makeInvestmentOverview } from "./factories";

vi.mock("vue-chartjs", async () => {
  const { LineChartStub } = await import("./line-chart-stub");
  return { Line: LineChartStub };
});

vi.mock("vue-router", () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({ push: vi.fn() }),
}));

const EMPTY_INVESTMENT_DEFAULTS = {
  investment_overview: makeInvestmentOverview(),
  list_instruments: { items: [], total: 0 },
  list_holdings: [],
  instrument_price_staleness: { stale_count: 0, threshold_days: 3 },
  cumulative_pnl_native_total: { total_cents: 0, native_currency: "CNY" },
  portfolio_value_trend: { currency_code: "CNY", points: [] },
  realized_pnl_summary: { by_year: [], by_account: [] },
  money_weighted_return_summary: { by_instrument: [], by_account: [], total: [] },
  list_investment_transactions: { items: [], total: 0 },
};

beforeEach(async () => {
  await wireInvokeSeam({ defaults: EMPTY_INVESTMENT_DEFAULTS, refreshReferenceStores: true }).ready;
});

afterEach(async () => {
  await applyLocale("zh-CN");
});

const mountView = () => mountWithDialog(InvestmentsView);

describe("InvestmentsView 英文渲染（issue #350 / ADR-0049）", () => {
  it("页签渲染英文：Overview / P&L / Holdings / Ledger / Instruments（Trend 随 #1907 退役）", async () => {
    await applyLocale("en-US");
    await nextTick();
    const wrapper = mountView();
    await nextTick();
    const labels = wrapper.findAll(".n-tabs-tab").map((el) => el.text());
    expect(labels).toContain("Overview");
    expect(labels).toContain("P&L");
    expect(labels).toContain("Holdings");
    expect(labels).toContain("Ledger");
    expect(labels).toContain("Instruments");
    expect(labels).not.toContain("Trend");
  });

  it("概览页签渲染英文（Investable assets / 两腿拆分 / 本位币标注）", async () => {
    await applyLocale("en-US");
    const wrapper = mountView();
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("Investment Overview");
    expect(text).toContain("Investable assets");
    expect(text).toContain("Investment account cash");
    expect(text).toContain("Holdings market value");
    expect(text).toContain("Native currency CNY");
  });

  it("持仓页签渲染英文（Current Holdings / 空态，issue #901）", async () => {
    await applyLocale("en-US");
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "Holdings");
    expect(wrapper.text()).toContain("Current Holdings");
    expect(wrapper.text()).toContain("Sync Instrument Info");
    expect(wrapper.text()).toContain("No holdings");
  });

  it("标的页工具栏与搜索框渲染英文", async () => {
    await applyLocale("en-US");
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "Instruments");
    expect(wrapper.text()).toContain("Holdings only");
    expect(wrapper.text()).toContain("Add Instrument");
    expect(wrapper.text()).not.toContain("Full Sync");
    expect(wrapper.find('input[placeholder="Search symbol or name..."]').exists()).toBe(true);
    const headers = wrapper.findAll("th").map((th) => th.text());
    expect(headers).toContain("Symbol");
    expect(headers).toContain("Price Source");
    expect(headers).toContain("Manual Price");
  });

  it("标的页「价格来源」列四值渲染英文（issue #1189 / 词汇表「价格通道」）", async () => {
    await applyLocale("en-US");
    const rows = [
      makeInstrument({
        id: "i-quote",
        symbol: "600000",
        type: "stock",
        source: "eastmoney",
        price_channel: "quote",
      }),
      makeInstrument({
        id: "i-nav",
        symbol: "000001",
        type: "fund",
        market: "unknown",
        source: "manual",
        price_channel: "fund_nav",
      }),
      makeInstrument({
        id: "i-manual",
        symbol: "HW-VR",
        type: "other",
        market: "unknown",
        source: "manual",
        price_channel: "manual",
      }),
      makeInstrument({
        id: "i-none",
        symbol: "ghost1",
        type: "stock",
        market: "unknown",
        source: "eastmoney",
        price_channel: "none",
      }),
    ];
    wireInvokeSeam({
      defaults: EMPTY_INVESTMENT_DEFAULTS,
      overrides: {
        list_instruments: () => Promise.resolve({ items: rows, total: rows.length }),
      },
    });
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "Instruments");
    const cells = wrapper.findAll('td[data-col-key="price_channel"]').map((c) => c.text());
    expect(cells).toEqual(["Quote", "NAV", "Manual Price", "No Source"]);
  });

  it("概览走势卡渲染英文（区间预设档位闭集与空态引导，#1907 起走势随概览）", async () => {
    await applyLocale("en-US");
    const wrapper = mountView();
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("Portfolio Trend");
    expect(text).toContain("Portfolio Value");
    expect(text).not.toContain("Single Instrument");
    for (const label of ["1M", "3M", "1Y", "3Y", "5Y", "All"]) {
      expect(text).toContain(label);
    }
    expect(text).toContain("No historical price data");
  });

  it("价格过期提示渲染英文（issue #1190：数量 / 阈值插值 + 去同步按钮）", async () => {
    await applyLocale("en-US");
    wireInvokeSeam({
      defaults: EMPTY_INVESTMENT_DEFAULTS,
      overrides: {
        instrument_price_staleness: () => ({ stale_count: 2, threshold_days: 3 }),
      },
    });
    const wrapper = mountView();
    await flushPromises();
    const alert = wrapper.find('[data-testid="price-staleness-alert"]');
    expect(alert.exists()).toBe(true);
    expect(alert.text()).toContain("2 instruments may have stale quotes / NAV");
    expect(alert.text()).toContain("over 3 days");
    expect(alert.text()).toContain("Sync Instrument Info");
  });

  it("投资表单渲染英文 label 与占位", async () => {
    await applyLocale("en-US");
    const wrapper = mount(InvestmentForm, {
      props: { kind: "buy", submitLabel: "记买入" },
    });
    const labels = wrapper.findAll(".n-form-item-label").map((el) => el.text());
    expect(labels).toContain("Amount");
    expect(labels).toContain("Investment Account");
    expect(labels).toContain("Instrument");
    expect(wrapper.text()).toContain("Select investment account");
  });
});
