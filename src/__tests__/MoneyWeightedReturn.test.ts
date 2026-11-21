import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { componentVm } from "@ledger/test-support/component-vm";
import { mountWithDialog } from "@ledger/test-support/mount";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import { hoverTipText } from "@ledger/test-support/tooltip";
import { firePricesChanged, resetPricesChangedHandler } from "./prices-changed-mock";
import { makeMwrSummary, makePnlSummary } from "./factories";
import RealizedPnlPanel from "@/investment/RealizedPnlPanel.vue";
import HoldingsOverview from "@/investment/HoldingsOverview.vue";

vi.mock("@/investment/usePricesChanged", async () => {
  const { capturePricesChangedHandler } = await import("./prices-changed-mock");
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  };
});

const HOLDINGS_DEFAULTS = {
  list_holdings: [
    {
      id: "h-1",
      account_id: "acc-1",
      instrument_id: "inst-1",
      quantity: 100,
      cost_basis_cents: 120000,
      cost_currency_code: "CNY",
      latest_price_cents: 150000,
      latest_price_currency_code: "CNY",
      latest_nav_date: null,
      market_value_cents: 150000,
      unrealized_pnl_cents: 30000,
      native_market_value_cents: 150000,
      native_unrealized_pnl_cents: 30000,
      updated_at: "2026-01-01T00:00:00Z",
    },
    {
      id: "h-2",
      account_id: "acc-1",
      instrument_id: "inst-2",
      quantity: 10,
      cost_basis_cents: 8000,
      cost_currency_code: "CNY",
      latest_price_cents: null,
      latest_price_currency_code: null,
      latest_nav_date: null,
      market_value_cents: null,
      unrealized_pnl_cents: null,
      updated_at: "2026-01-01T00:00:00Z",
    },
  ],
  list_instruments: {
    items: [
      { id: "inst-1", symbol: "600000", name: "浦发银行", price_channel: "quote" },
      { id: "inst-2", symbol: "000001", name: "平安银行", price_channel: "quote" },
    ],
    total: 2,
  },
  cumulative_pnl_native_total: { total_cents: 48000, native_currency: "CNY" },
  sync_instrument_info: { synced: 0, skipped: 0, message: "" },
};

beforeEach(async () => {
  resetPricesChangedHandler();
  await wireInvokeSeam({ defaults: HOLDINGS_DEFAULTS, refreshReferenceStores: true }).ready;
});

afterEach(() => {
  resetPricesChangedHandler();
});

function mwrCellTexts(
  wrapper: ReturnType<typeof mount>,
  colKey: "rate" | "mwr" = "rate",
): string[] {
  return wrapper.findAll(`td[data-col-key="${colKey}"]`).map((c) => c.text());
}

describe("资金加权收益率前端接线（issue #1195 / ADR-0115）", () => {
  it("持仓页签收益率列：可计算行渲染带色百分数，缺价行「-」，无解行「无法计算」", async () => {
    wireInvokeSeam({
      defaults: {
        ...HOLDINGS_DEFAULTS,
        money_weighted_return_summary: makeMwrSummary({
          by_instrument: [
            {
              account_id: "acc-1",
              instrument_id: "inst-1",
              currency_code: "CNY",
              basis: "annualized",
              rate: 0.1,
            },
            {
              account_id: "acc-1",
              instrument_id: "inst-2",
              currency_code: "CNY",
              basis: "annualized",
              rate: null,
            },
          ],
        }),
      },
    });
    const wrapper = mountWithDialog(HoldingsOverview);
    await flushPromises();

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "money_weighted_return_summary")).toBe(
      true,
    );
    const headers = wrapper.findAll("th").map((th) => th.text());
    expect(headers).toContain("资金加权收益率");
    const cells = wrapper.findAll('td[data-col-key="mwr"]').map((c) => c.text());
    expect(cells).toHaveLength(2);
    expect(cells[0]).toBe("无法计算");
    expect(cells[1]).toBe("+10.00%");

    wireInvokeSeam({
      defaults: HOLDINGS_DEFAULTS,
      overrides: {
        money_weighted_return_summary: makeMwrSummary({
          by_instrument: [
            {
              account_id: "acc-1",
              instrument_id: "inst-1",
              currency_code: "CNY",
              basis: "annualized",
              rate: 0.1,
            },
          ],
        }),
      },
    });
    const wrapper2 = mountWithDialog(HoldingsOverview);
    await flushPromises();
    const cells2 = wrapper2.findAll('td[data-col-key="mwr"]').map((c) => c.text());
    expect(cells2).toEqual(["-", "+10.00%"]);
    wrapper.unmount();
    wrapper2.unmount();
  });

  it("期初存量标的：按行口径带「*」角标，年化标的保持原样（issue #1343）", async () => {
    wireInvokeSeam({
      defaults: {
        ...HOLDINGS_DEFAULTS,
        money_weighted_return_summary: makeMwrSummary({
          by_instrument: [
            {
              account_id: "acc-1",
              instrument_id: "inst-1",
              currency_code: "CNY",
              basis: "cumulative",
              rate: 0.0316,
            },
            {
              account_id: "acc-1",
              instrument_id: "inst-2",
              currency_code: "CNY",
              basis: "annualized",
              rate: 0.1,
            },
          ],
        }),
      },
    });
    const wrapper = mountWithDialog(HoldingsOverview);
    await flushPromises();

    expect(mwrCellTexts(wrapper, "mwr")).toEqual(["+10.00%", "+3.16%*"]);
    wrapper.unmount();
  });

  it("未年化角标的解释双轴可达：指针悬停出 tooltip、触控点按出气泡（ADR-0088 决策 6）", async () => {
    wireInvokeSeam({
      defaults: {
        ...HOLDINGS_DEFAULTS,
        money_weighted_return_summary: makeMwrSummary({
          by_instrument: [
            {
              account_id: "acc-1",
              instrument_id: "inst-1",
              currency_code: "CNY",
              basis: "cumulative",
              rate: 0.0316,
            },
          ],
        }),
      },
    });
    const wrapper = mountWithDialog(HoldingsOverview);
    await flushPromises();
    const trigger = wrapper.find('td[data-col-key="mwr"] [data-mwr-marker]');
    expect(trigger.exists()).toBe(true);
    const tip = await hoverTipText(trigger);
    expect(tip).toContain("未年化");
    expect(tip).toContain("累计收益");
    wrapper.unmount();

    setFakeMedia({ width: 600, hover: "none", pointer: "coarse" });
    const touchWrapper = mountWithDialog(HoldingsOverview);
    await flushPromises();
    const touchTrigger = touchWrapper.find('td[data-col-key="mwr"] [data-mwr-marker]');
    expect(touchTrigger.exists()).toBe(true);
    await touchTrigger.trigger("click");
    await flushPromises();
    const popover = document.body.querySelector(".n-popover");
    expect(popover).not.toBeNull();
    expect(popover!.textContent).toContain("未年化");
    touchWrapper.unmount();
  });

  it("盈亏页资金加权收益率卡：账户行 + 全账行（按币种分组），无解显式标注", async () => {
    wireInvokeSeam({
      defaults: {
        realized_pnl_summary: makePnlSummary(),
        money_weighted_return_summary: makeMwrSummary({
          by_account: [
            {
              account_id: "acc-1",
              account_name: "证券账户A",
              currency_code: "CNY",
              basis: "annualized",
              rate: 0.1,
            },
          ],
          total: [
            { currency_code: "CNY", basis: "annualized", rate: 0.08 },
            { currency_code: "USD", basis: "annualized", rate: null },
          ],
        }),
      },
    });
    const wrapper = mountWithDialog(RealizedPnlPanel);
    await flushPromises();

    expect(wrapper.text()).toContain("资金加权收益率");
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "money_weighted_return_summary")).toBe(
      true,
    );
    expect(mwrCellTexts(wrapper)).toEqual(["+10.00%", "+8.00%", "无法计算"]);
    const scopeTexts = wrapper.findAll('td[data-col-key="scope"]').map((c) => c.text());
    expect(scopeTexts).toEqual(["证券账户A", "全账", "全账"]);
    const currencyTexts = wrapper.findAll('td[data-col-key="currency_code"]').map((c) => c.text());
    expect(currencyTexts).toEqual(["CNY", "CNY", "USD"]);
    wrapper.unmount();
  });

  it("口径说明接线：已实现收益两表列头与收益率卡各带说明触发器（ADR-0129 / issue #1369）", async () => {
    wireInvokeSeam({
      defaults: {
        realized_pnl_summary: makePnlSummary(),
        money_weighted_return_summary: makeMwrSummary(),
      },
    });
    const wrapper = mountWithDialog(RealizedPnlPanel);
    await flushPromises();
    for (const id of ["pnl-realized-gain-info", "pnl-mwr-info", "pnl-mwr-card-info"]) {
      expect(wrapper.find(`[data-testid="${id}"]`).exists(), id).toBe(true);
    }
    const tip = await hoverTipText(wrapper.find('[data-testid="pnl-realized-gain-info"]'));
    expect(tip).toContain("已实现盈亏 + 现金分红");
    expect(tip).toContain("不含仍在持仓里的浮动盈亏");
    wrapper.unmount();
  });

  it("盈亏页合集含期初存量：账户级与全账级整项标未年化（issue #1346）", async () => {
    wireInvokeSeam({
      defaults: {
        realized_pnl_summary: makePnlSummary(),
        money_weighted_return_summary: makeMwrSummary({
          by_account: [
            {
              account_id: "acc-1",
              account_name: "雪球基金",
              currency_code: "CNY",
              basis: "cumulative",
              rate: 0.0316,
            },
          ],
          total: [{ currency_code: "CNY", basis: "cumulative", rate: 0.028 }],
        }),
      },
    });
    const wrapper = mountWithDialog(RealizedPnlPanel);
    await flushPromises();

    expect(mwrCellTexts(wrapper)).toEqual(["+3.16%*", "+2.80%*"]);
    const scopeTexts = wrapper.findAll('td[data-col-key="scope"]').map((c) => c.text());
    expect(scopeTexts).toEqual(["雪球基金", "全账"]);
    expect(wrapper.findAll('td[data-col-key="rate"] [data-mwr-marker]')).toHaveLength(2);
    wrapper.unmount();
  });

  it("盈亏页账户筛选收窄账户行，全账行保持全账本口径", async () => {
    wireInvokeSeam({
      defaults: {
        realized_pnl_summary: makePnlSummary(),
        money_weighted_return_summary: makeMwrSummary({
          by_account: [
            {
              account_id: "acc-1",
              account_name: "证券账户A",
              currency_code: "CNY",
              basis: "annualized",
              rate: 0.1,
            },
            {
              account_id: "acc-2",
              account_name: "基金账户B",
              currency_code: "CNY",
              basis: "annualized",
              rate: -0.05,
            },
          ],
        }),
      },
    });
    const wrapper = mountWithDialog(RealizedPnlPanel);
    await flushPromises();
    expect(await mwrCellTexts(wrapper)).toEqual(["+10.00%", "-5.00%", "+10.00%"]);

    const select = wrapper.findComponent({ name: "PinyinSelect" });
    componentVm(select).$emit("update:value", "acc-1");
    await nextTick();
    const scopeTexts = wrapper.findAll('td[data-col-key="scope"]').map((c) => c.text());
    expect(scopeTexts).toEqual(["证券账户A", "全账"]);
    wrapper.unmount();
  });

  it("价格失效信号驱动重拉（期末市值随行情变动）", async () => {
    wireInvokeSeam({
      defaults: {
        realized_pnl_summary: makePnlSummary(),
        money_weighted_return_summary: makeMwrSummary(),
      },
    });
    const wrapper = mountWithDialog(RealizedPnlPanel);
    await flushPromises();
    const callsBefore = mockInvoke.mock.calls.filter(
      ([cmd]) => cmd === "money_weighted_return_summary",
    ).length;
    expect(callsBefore).toBeGreaterThanOrEqual(1);

    firePricesChanged();
    await flushPromises();
    const callsAfter = mockInvoke.mock.calls.filter(
      ([cmd]) => cmd === "money_weighted_return_summary",
    ).length;
    expect(callsAfter).toBeGreaterThan(callsBefore);
    wrapper.unmount();
  });
});
