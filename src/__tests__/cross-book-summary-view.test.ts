import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { hoverTipText } from "@ledger/test-support/tooltip";
import { formatAmount } from "@ledger/money";
import CrossBookSummaryView from "@/views/CrossBookSummaryView.vue";
import type { Currency, CrossBookInvestmentSummary } from "@ledger/types";

const CNY: Currency = { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 };

function makeSummary(
  overrides: Partial<CrossBookInvestmentSummary> = {},
): CrossBookInvestmentSummary {
  return {
    target_currency: "CNY",
    converted: true,
    market_value_cents: 120_000,
    unrealized_pnl_cents: 20_000,
    cumulative_pnl_cents: 45_000,
    investable_assets_cents: 190_000,
    books: [
      { id: "b1", name: "默认账本", status: "included" },
      { id: "b2", name: "美股账本", status: "included" },
      { id: "b3", name: "密码本", status: "locked" },
      { id: "b4", name: "空账本", status: "not_initialized" },
    ],
    ...overrides,
  };
}

let payload: CrossBookInvestmentSummary | null = makeSummary();
let fail = false;

function wire(): void {
  wireInvokeSeam({
    overrides: {
      cross_book_investment_summary: () => {
        if (fail) throw new Error("未找到 USD -> CNY 的汇率（正反向均无）");
        return payload;
      },
    },
  });
}

beforeEach(() => {
  payload = makeSummary();
  fail = false;
  wire();
});

afterEach(() => {
  payload = null;
});

async function mountView() {
  const wrapper = mount(CrossBookSummaryView);
  await flushPromises();
  return wrapper;
}

describe("跨账本投资汇总视图", () => {
  it("渲染四个口径合计（金额经 formatAmount、目标币种符号）", async () => {
    const wrapper = await mountView();
    expect(wrapper.find('[data-testid="summary-marketValue"]').text()).toBe(
      formatAmount(120_000, CNY),
    );
    expect(wrapper.find('[data-testid="summary-unrealizedPnl"]').text()).toBe(
      formatAmount(20_000, CNY),
    );
    expect(wrapper.find('[data-testid="summary-cumulativePnl"]').text()).toBe(
      formatAmount(45_000, CNY),
    );
    expect(wrapper.find('[data-testid="summary-investableAssets"]').text()).toBe(
      formatAmount(190_000, CNY),
    );
  });

  it("发生过折算时标注当期汇率口径（ADR-0114 决策 3 界面义务）", async () => {
    const wrapper = await mountView();
    expect(wrapper.text()).toContain("已按当期汇率折算为 CNY");
    expect(wrapper.text()).not.toContain("直接相加");
  });

  it("位币一致（未折算）时标注直接相加口径", async () => {
    payload = makeSummary({ converted: false });
    wire();
    const wrapper = await mountView();
    expect(wrapper.text()).toContain("直接相加（CNY）");
  });

  it("存在未计入本时警示部分合计，并逐本呈现状态（ADR-0114 决策 5）", async () => {
    const wrapper = await mountView();
    expect(wrapper.text()).toContain("部分账本未计入");
    expect(wrapper.text()).toContain("未解锁、未计入");
    expect(wrapper.text()).toContain("尚未初始化、未计入");
    expect(wrapper.find('[data-testid="summary-book-b3"]').text()).toContain("密码本");
  });

  it("全部计入时不出现部分合计警示", async () => {
    payload = makeSummary({
      books: [
        { id: "b1", name: "默认账本", status: "included" },
        { id: "b2", name: "美股账本", status: "included" },
      ],
    });
    wire();
    const wrapper = await mountView();
    expect(wrapper.text()).not.toContain("部分账本未计入");
  });

  it("命令报错转 error 兜底显示中文错误，重试可恢复（不空数字、不崩溃）", async () => {
    fail = true;
    const wrapper = await mountView();
    expect(wrapper.text()).toContain("汇总读取失败");
    expect(wrapper.text()).toContain("未找到 USD -> CNY 的汇率（正反向均无）");
    expect(wrapper.find('[data-testid="summary-marketValue"]').exists()).toBe(false);

    fail = false;
    wire();
    await wrapper.find("button").trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="summary-marketValue"]').text()).toBe(
      formatAmount(120_000, CNY),
    );
  });

  it("四个合计口径各带说明触发器，并挂跨本作用域句（issue #1369）", async () => {
    const wrapper = await mountView();
    for (const key of ["marketValue", "unrealizedPnl", "cumulativePnl", "investableAssets"]) {
      const trigger = wrapper.find(`[data-testid="cross-book-${key}-info"]`);
      expect(trigger.exists(), key).toBe(true);
      expect(trigger.attributes("aria-label")).toContain("说明");
    }
    const tip = await hoverTipText(
      wrapper.find('[data-testid="cross-book-investableAssets-info"]'),
    );
    expect(tip).toContain("隐藏账户与负债都不计入");
    expect(tip).toContain("未解锁或未建库的账本不计入");
    wrapper.unmount();
  });
});

import { routes } from "@/router/index";

describe("跨账本投资汇总路由接线", () => {
  it("路由表存在 cross-book-summary 记录并指向本视图", () => {
    const route = routes.find((r) => r.name === "cross-book-summary");
    expect(route).toBeDefined();
    expect(String(route?.path)).toBe("/cross-book-summary");
    expect(String(route?.component)).toContain("CrossBookSummaryView");
  });
});
