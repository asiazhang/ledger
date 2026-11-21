import { describe, it, expect, vi, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { findButton, probeColor } from "@ledger/test-support/dom";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import { hoverTipText } from "@ledger/test-support/tooltip";
import { nextTick } from "vue";
import { applyLocale } from "@ledger/i18n";
import DashboardView from "@/views/DashboardView.vue";
import TransactionForm from "@/transaction/TransactionForm.vue";
import { amountPrivacyEnabled, formatAmount } from "@ledger/money";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import { pnlSemanticColor } from "@ledger/theme/semantic-colors";
import { NGrid, NProgress } from "naive-ui";
import {
  makeAccount,
  makeFinancialFreedom,
  makeHolding,
  makeOverview,
  mockHoldings,
  mockInstruments,
} from "./factories";

const pushMock = vi.fn();
vi.mock("vue-router", () => ({
  useRouter: () => ({ push: pushMock }),
}));
import type { Account, Currency, MonthlySummary } from "@ledger/types";

const mockCurrencies: Currency[] = [
  { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 },
  { code: "USD", name: "美元", symbol: "$", decimal_places: 2 },
];

const [cny] = mockCurrencies;

const mockAccounts: Account[] = [
  {
    id: "acc-1",
    name: "现金",
    type: "cash",
    currency_code: "CNY",
    initial_balance_cents: 10000,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    is_hidden: false,
    is_archived: false,
  },
];

const mockOverview = makeOverview();

let mockMonthlySummary: MonthlySummary[] = [];

function setCurrentMonthSummary(summary: Omit<MonthlySummary, "month">) {
  const now = new Date();
  const monthKey = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
  mockMonthlySummary = [{ month: monthKey, ...summary }];
}

const BASE_DEFAULTS = {
  list_holdings: mockHoldings,
  list_instruments: { items: mockInstruments, total: mockInstruments.length },
  cumulative_pnl_native_total: { total_cents: 48000, native_currency: "CNY" },
  dashboard_overview: mockOverview,
  financial_freedom: makeFinancialFreedom(),
  budget_annual_summary: {
    annual_budget_total_cents: 1_200_000,
    spent_cents: 300_000,
    over_budget: false,
  },
};

const BASE_OVERRIDES = {
  list_currencies: mockCurrencies,
  list_categories: [],
  monthly_summary: () => mockMonthlySummary,
};

beforeEach(async () => {
  pushMock.mockClear();
  mockMonthlySummary = [];
  wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: BASE_OVERRIDES });
  const store = useReferenceStore();
  await store.refresh();
});

async function mountView() {
  const wrapper = mount(DashboardView);
  await flushPromises();
  return wrapper;
}

describe("DashboardView 界面语言切换（issue #342 / #351）", () => {
  it("en-US 下卡片标题与标签渲染英文文案，切回 zh-CN 恢复中文", async () => {
    try {
      await applyLocale("en-US");
      await nextTick();
      const wrapper = await mountView();
      const text = wrapper.text();
      expect(text).toContain("Net Worth");
      expect(text).toContain("This Month");
      expect(text).toContain("Income");
      expect(text).toContain("Net Expense");
      expect(wrapper.find('[data-testid="investment-overview-card"]').text()).toContain(
        "Total Market Value",
      );
      expect(wrapper.find('[data-testid="investment-overview-card"]').text()).toContain(
        "Holding P&L",
      );
      expect(wrapper.find('[data-testid="dashboard-total-cumulative-pnl"]').text()).toContain(
        "Cumulative P&L",
      );
      expect(wrapper.find('[data-testid="financial-freedom-card"]').text()).toContain(
        "Financial Freedom",
      );
      expect(wrapper.find('[data-testid="financial-freedom-stage"]').text()).toContain(
        "Accumulating",
      );
      expect(wrapper.find('[data-testid="financial-freedom-card"]').text()).not.toContain("积累期");
    } finally {
      await applyLocale("zh-CN");
      await nextTick();
    }
  });
});

describe("DashboardView 净资产总览卡（issue #143）", () => {
  it("首页顶部呈现净资产总览卡：本位币单一主数字", async () => {
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="net-worth-card"]');
    expect(card.exists()).toBe(true);
    expect(card.text()).toContain("净资产");
    expect(card.text()).toContain(formatAmount(123456, cny));
    expect(card.find('[data-testid="net-worth-info"]').exists()).toBe(true);
  });

  it("卡内下钻：点「查看账户明细」跳账户页（总览屏唯一动作）", async () => {
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="net-worth-card"]');
    const action = card.find('[data-testid="net-worth-view-accounts"]');
    expect(action.exists()).toBe(true);
    await action.trigger("click");
    expect(pushMock).toHaveBeenCalledWith({ name: "accounts" });
  });

  it("命令报错（如缺汇率）时卡片显示提示文案而非空数字或崩溃", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...BASE_OVERRIDES,
        dashboard_overview: () => Promise.reject(new Error("缺少 USD→CNY 汇率，无法折算")),
      },
    });
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="net-worth-card"]');
    expect(card.text()).toContain("净资产");
    expect(card.text()).toContain("缺少 USD→CNY 汇率，无法折算");
    expect(card.text()).not.toContain(formatAmount(0, cny));
  });
});

describe("DashboardView 投资概览卡（issue #145）", () => {
  it("有持仓时展示总市值、持仓收益与累计收益，无行情行不以零计入", async () => {
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="investment-overview-card"]');
    expect(card.exists()).toBe(true);
    expect(card.text()).toContain("总市值");
    expect(card.text()).toContain(formatAmount(150000, cny));
    expect(card.find('[data-testid="dashboard-total-unrealized-pnl-value"]').text()).toBe(
      formatAmount(30000, cny),
    );
    expect(card.find('[data-testid="dashboard-total-unrealized-pnl-missing-price"]').text()).toBe(
      "1 只持仓因缺现价未计入。",
    );
    expect(card.find('[data-testid="dashboard-total-cumulative-pnl"]').text()).toBe(
      `累计收益${formatAmount(48000, cny)}`,
    );
    expect(card.text()).not.toContain(formatAmount(0, cny));
    expect(card.text()).not.toContain("未实现盈亏");
  });

  it("投资概览三卡：盈亏两卡按符号着盈亏涨跌色、市值卡不着色，三卡各带口径说明", async () => {
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="investment-overview-card"]');
    const theme = useAppStore().theme;
    const inlineColors = (testId: string) =>
      card
        .findAll(`[data-testid="${testId}-value"] span[style]`)
        .map((s) => (s.element as HTMLElement).style.color);
    expect(inlineColors("dashboard-total-market-value")).toEqual([]);
    expect(inlineColors("dashboard-total-unrealized-pnl")).toEqual([
      probeColor(pnlSemanticColor(30000, theme)),
    ]);
    expect(inlineColors("dashboard-total-cumulative-pnl")).toEqual([
      probeColor(pnlSemanticColor(48000, theme)),
    ]);
    expect(card.findAll(".n-statistic-value")).toHaveLength(3);
    for (const id of [
      "dashboard-total-market-value",
      "dashboard-total-unrealized-pnl",
      "dashboard-total-cumulative-pnl",
    ]) {
      expect(card.find(`[data-testid="${id}-info"]`).exists(), id).toBe(true);
    }
    const tip = await hoverTipText(
      card.find('[data-testid="dashboard-total-cumulative-pnl-info"]'),
    );
    expect(tip).toContain("累计分红");
  });

  it("投资概览卡口径说明挂 wholeLedger 作用域句（与持仓页 filtered 变体分家，issue #1369）", async () => {
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="investment-overview-card"]');
    const tip = await hoverTipText(card.find('[data-testid="dashboard-total-market-value-info"]'));
    expect(tip).toContain("覆盖全账本的持仓");
    expect(tip).not.toContain("随当前搜索与账户过滤收窄");
    wrapper.unmount();
  });

  it("多币种持仓折本位币单值：不再分组拼接，累计收益同单值（issue #1797）", async () => {
    const usdAccount = makeAccount({ id: "acc-2", name: "美股账户", currency_code: "USD" });
    const usdHolding = makeHolding({
      id: "h-3",
      instrument_id: "inst-2",
      account_id: "acc-2",
      cost_currency_code: "USD",
      latest_price_cents: 3000,
      latest_price_currency_code: "USD",
      market_value_cents: 3000,
      unrealized_pnl_cents: -500,
      native_market_value_cents: 21000,
      native_unrealized_pnl_cents: -3500,
    });
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...BASE_OVERRIDES,
        list_holdings: [mockHoldings[0], usdHolding],
        list_accounts: [mockAccounts[0], usdAccount],
      },
    });
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="investment-overview-card"]');
    expect(card.find('[data-testid="dashboard-total-market-value-value"]').text()).toBe(
      formatAmount(171000, cny),
    );
    expect(card.find('[data-testid="dashboard-total-unrealized-pnl-value"]').text()).toBe(
      formatAmount(26500, cny),
    );
    expect(card.find('[data-testid="dashboard-total-cumulative-pnl-value"]').text()).toBe(
      formatAmount(48000, cny),
    );
    expect(card.text()).not.toContain(" / ");
  });

  it("无任何持仓时卡片保留，空态占位而非统计数字", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...BASE_OVERRIDES,
        list_holdings: [],
        list_instruments: { items: [], total: 0 },
      },
    });
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="investment-overview-card"]');
    expect(card.exists()).toBe(true);
    expect(card.text()).toContain("暂无持仓");
    expect(card.text()).not.toContain("总市值");
  });

  it("有持仓但全部无行情时空值分支：合计统计降级为「-」并标注缺价计数", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...BASE_OVERRIDES,
        list_holdings: [mockHoldings[1]],
        list_instruments: { items: [mockInstruments[1]], total: 1 },
        cumulative_pnl_native_total: { total_cents: 0, native_currency: "CNY" },
      },
    });
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="investment-overview-card"]');
    expect(card.exists()).toBe(true);
    expect(card.text()).toContain("总市值");
    expect(card.find('[data-testid="dashboard-total-market-value-value"]').text()).toBe("-");
    expect(card.find('[data-testid="dashboard-total-unrealized-pnl-value"]').text()).toBe("-");
    expect(card.find('[data-testid="dashboard-total-market-value-missing-price"]').text()).toBe(
      "1 只持仓因缺现价未计入。",
    );
    expect(card.find('[data-testid="dashboard-total-cumulative-pnl-value"]').text()).toBe(
      formatAmount(0, cny),
    );
  });
});

describe("DashboardView 本年度预算卡（年度汇总口径）", () => {
  it("展示年度预算总额与本年已支、正常态标签与 25% 进度；位于投资概览之后、自由度之前", async () => {
    const wrapper = await mountView();
    const ids = wrapper.findAll('[data-testid$="-card"]').map((w) => w.attributes("data-testid"));
    const budgetIdx = ids.indexOf("dashboard-budget-card");
    expect(budgetIdx).toBeGreaterThan(ids.indexOf("investment-overview-card"));
    expect(budgetIdx).toBeLessThan(ids.indexOf("financial-freedom-card"));

    const card = wrapper.find('[data-testid="dashboard-budget-card"]');
    expect(card.find('[data-testid="dashboard-budget-total"]').text()).toBe(
      formatAmount(1_200_000),
    );
    expect(card.find('[data-testid="dashboard-budget-spent"]').text()).toBe(formatAmount(300_000));
    expect(card.find('[data-testid="dashboard-budget-stage"]').text()).toBe("正常");
    const bar = card.findComponent(NProgress);
    expect(bar.props("percentage")).toBe(25);
    expect(bar.props("status")).toBe("default");
    expect(card.text()).toContain("月预算按 ×12 年化计入总额");
  });

  it("超标时标签转已超标、进度条转错误色", async () => {
    wireInvokeSeam({
      defaults: {
        ...BASE_DEFAULTS,
        budget_annual_summary: {
          annual_budget_total_cents: 1_000_000,
          spent_cents: 1_500_000,
          over_budget: true,
        },
      },
      overrides: BASE_OVERRIDES,
    });
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="dashboard-budget-card"]');
    expect(card.find('[data-testid="dashboard-budget-stage"]').text()).toBe("已超标");
    expect(card.findComponent(NProgress).props("status")).toBe("error");
    expect(card.findComponent(NProgress).props("percentage")).toBe(100);
  });

  it("命令报错时卡内警告而非半截数字（重试即再取）", async () => {
    const retry = vi.fn(() => Promise.reject(new Error("本年预算汇总读取失败")));
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { ...BASE_OVERRIDES, budget_annual_summary: retry },
    });
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="dashboard-budget-card"]');
    expect(card.text()).toContain("本年预算汇总读取失败");
    expect(card.find('[data-testid="dashboard-budget-total"]').exists()).toBe(false);
  });
});

describe("DashboardView 财务自由度卡（issue #344）", () => {
  it("卡片位于投资概览卡之后（末位）", async () => {
    const wrapper = await mountView();
    const ids = wrapper.findAll('[data-testid$="-card"]').map((w) => w.attributes("data-testid"));
    const freedomIdx = ids.indexOf("financial-freedom-card");
    expect(freedomIdx).toBeGreaterThan(ids.indexOf("investment-overview-card"));
  });

  it("展示大字百分比、进度条、分子/分母金额、覆盖年数与阶段标签", async () => {
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="financial-freedom-card"]');
    expect(card.find('[data-testid="financial-freedom-ratio"]').text()).toBe("7.5%");
    expect(card.find('[data-testid="financial-freedom-stage"]').text()).toBe("积累期");
    expect(card.text()).toContain(`可投资资产 ${formatAmount(500000, cny)}`);
    expect(card.text()).toContain(`年度预算总额 ${formatAmount(2000000, cny)}`);
    expect(card.text()).toContain("可覆盖 0.3 年");
    const bar = card.findComponent(NProgress);
    expect(bar.props("percentage")).toBe(7.5);
    expect(bar.props("status")).not.toBe("success");
  });

  it("阶段标签三档边界：<30% 积累期 / 30–100% 接近自由 / ≥100% 财务自由", async () => {
    const stageOf = async (ratio: number) => {
      wireInvokeSeam({
        defaults: BASE_DEFAULTS,
        overrides: { ...BASE_OVERRIDES, financial_freedom: makeFinancialFreedom({ ratio }) },
      });
      const wrapper = await mountView();
      return wrapper.find('[data-testid="financial-freedom-stage"]').text();
    };
    expect(await stageOf(29.9)).toBe("积累期");
    expect(await stageOf(30)).toBe("接近自由");
    expect(await stageOf(99.9)).toBe("接近自由");
    expect(await stageOf(100)).toBe("财务自由");
  });

  it("≥100% 进度条封顶 100；>100% 百分比原文呈现", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { ...BASE_OVERRIDES, financial_freedom: makeFinancialFreedom({ ratio: 150 }) },
    });
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="financial-freedom-card"]');
    expect(card.find('[data-testid="financial-freedom-ratio"]').text()).toBe("150%");
    const bar = card.findComponent(NProgress);
    expect(bar.props("status")).toBe("default");
    expect(bar.props("percentage")).toBe(100);
  });

  it("零分母（未设预算）显示占位引导，点击跳转预算页；不回退实际支出", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...BASE_OVERRIDES,
        financial_freedom: makeFinancialFreedom({
          ratio: 0,
          denominator_cents: 0,
          coverage_years: 0,
        }),
      },
    });
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="financial-freedom-card"]');
    expect(card.text()).toContain("设置预算后解锁财务自由度");
    expect(card.text()).not.toContain("%");
    expect(card.text()).not.toContain(cny.symbol);
    expect(card.findComponent(NProgress).exists()).toBe(false);
    const btn = findButton(card, "去设置预算", { exact: true });
    expect(btn).toBeTruthy();
    await btn!.trigger("click");
    expect(pushMock).toHaveBeenCalledWith({ name: "budget" });
  });

  it("零资产显示 0%（起点清晰可见而非功能消失）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...BASE_OVERRIDES,
        financial_freedom: makeFinancialFreedom({ ratio: 0, numerator_cents: 0 }),
      },
    });
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="financial-freedom-card"]');
    expect(card.find('[data-testid="financial-freedom-ratio"]').text()).toBe("0%");
    expect(card.find('[data-testid="financial-freedom-stage"]').text()).toBe("积累期");
  });

  it("缺汇率时卡内警告提示，可重试恢复", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...BASE_OVERRIDES,
        financial_freedom: () => Promise.reject(new Error("缺少 USD→CNY 汇率，无法折算")),
      },
    });
    const wrapper = await mountView();
    let card = wrapper.find('[data-testid="financial-freedom-card"]');
    expect(card.text()).toContain("缺少 USD→CNY 汇率，无法折算");
    expect(card.text()).not.toContain("%");

    wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: BASE_OVERRIDES });
    const retry = findButton(card, "重试", { exact: true });
    expect(retry).toBeTruthy();
    await retry!.trigger("click");
    await flushPromises();
    card = wrapper.find('[data-testid="financial-freedom-card"]');
    expect(card.find('[data-testid="financial-freedom-ratio"]').text()).toBe("7.5%");
  });
});

describe("DashboardView 财务自由度卡计算口径提示（tooltip）", () => {
  it("标题旁信息图标悬停展示计算口径：公式、分子/分母构成与 3% 提取率", async () => {
    const wrapper = await mountView();
    const card = wrapper.find('[data-testid="financial-freedom-card"]');
    const trigger = card.find('[data-testid="financial-freedom-info"]');
    expect(trigger.exists()).toBe(true);
    expect(document.body.textContent).not.toContain("安全提取率");

    const text = await hoverTipText(trigger);
    expect(text).toContain("可投资资产 × 3% ÷ 年度预算总额");
    expect(text).toContain("持仓市值 + 投资账户余额");
    expect(text).toContain("月度预算 × 12 + 年度预算");
    expect(text).toContain("安全提取率");
  });
});

describe("DashboardView 快速记账与最近交易移除（issue #141）", () => {
  it("不再渲染快速记账表单（TransactionForm）", async () => {
    const wrapper = await mountView();
    expect(wrapper.findComponent(TransactionForm).exists()).toBe(false);
    expect(wrapper.text()).not.toContain("快速记账");
  });

  it("不再渲染最近交易列表", async () => {
    const wrapper = await mountView();
    expect(wrapper.text()).not.toContain("最近交易");
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "list_transactions")).toBe(false);
  });

  it("不再渲染逐账户余额卡片（逐账户明细归账户页，首页只呈现聚合全貌）", async () => {
    const wrapper = await mountView();
    expect(wrapper.text()).not.toContain("现金");
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "list_account_balances")).toBe(false);
  });
});

describe("DashboardView 本月收支卡（issue #144）", () => {
  it("三格口径：收入=净收入、净支出=毛支出−退款、结余=收入−净支出", async () => {
    setCurrentMonthSummary({ income_cents: 100000, expense_cents: 80000, refund_cents: 5000 });
    const wrapper = await mountView();
    const text = wrapper.text();
    expect(text).toContain("本月收支");
    expect(text).toContain(`收入${formatAmount(100000)}`);
    expect(text).toContain(`净支出${formatAmount(75000)}`);
    expect(text).toContain(`结余${formatAmount(25000)}`);
  });

  it("净支出与预算消耗、分类占比口径一致（退款冲减而非单列）", async () => {
    setCurrentMonthSummary({ income_cents: 0, expense_cents: 12345, refund_cents: 2345 });
    const wrapper = await mountView();
    expect(wrapper.text()).toContain(`净支出${formatAmount(10000)}`);
  });

  it("当月无交易行时三格显示 0", async () => {
    mockMonthlySummary = [
      { month: "1999-01", income_cents: 999, expense_cents: 888, refund_cents: 7 },
    ];
    const wrapper = await mountView();
    expect(wrapper.text()).toContain(
      `本月收支收入${formatAmount(0)}净支出${formatAmount(0)}结余${formatAmount(0)}`,
    );
  });
});

describe("DashboardView 移动档（issue #847 / ADR-0088 决策 11 票⑦，词汇表「窗口分级」）", () => {
  it("移动档栅格单列化：总览两卡、本月收支三格、投资概览三格均单列（断点口径接窗口分级常量，不自立断点）", async () => {
    setFakeMedia({ width: 600 });
    const wrapper = await mountView();
    const grids = wrapper.findAllComponents(NGrid);
    expect(grids.length).toBe(3);
    expect(grids[0].props("cols")).toBe(1);
    expect(grids[1].props("cols")).toBe(1);
    expect(grids[2].props("cols")).toBe(1);
  });

  it('桌面档栅格：总览两卡两列、本月收支三列、投资概览三列（列数纯数字——具名断点写法在 responsive="self" 下会静默退成 1 列）', async () => {
    const wrapper = await mountView();
    const grids = wrapper.findAllComponents(NGrid);
    expect(grids.length).toBe(3);
    expect(grids[0].props("cols")).toBe(2);
    expect(grids[1].props("cols")).toBe(3);
    expect(grids[2].props("cols")).toBe(3);
  });

  it("跨断点缩窗实时切列（单列 ⇄ 两列 / 三列）", async () => {
    const wrapper = await mountView();
    expect(wrapper.findAllComponents(NGrid)[1].props("cols")).toBe(3);
    setFakeMedia({ width: 600 });
    await flushPromises();
    expect(wrapper.findAllComponents(NGrid)[0].props("cols")).toBe(1);
    expect(wrapper.findAllComponents(NGrid)[1].props("cols")).toBe(1);
    setFakeMedia({ width: 1280 });
    await flushPromises();
    expect(wrapper.findAllComponents(NGrid)[0].props("cols")).toBe(2);
    expect(wrapper.findAllComponents(NGrid)[1].props("cols")).toBe(3);
  });

  it("移动档触控轴：自由度口径说明点按可达（气泡全文）；指针轴点击无气泡（行为不变）", async () => {
    setFakeMedia({ width: 600, hover: "none", pointer: "coarse" });
    const touch = await mountView();
    const trigger = touch.find('[data-testid="financial-freedom-info"]');
    expect(trigger.exists()).toBe(true);
    expect((trigger.element as HTMLElement).style.getPropertyValue("--touch-hit-inset")).toBe(
      "-8px -14px",
    );
    expect(document.body.querySelector(".n-popover")).toBeNull();
    await trigger.trigger("click");
    await flushPromises();
    const popover = document.body.querySelector(".n-popover");
    expect(popover).not.toBeNull();
    expect(popover!.textContent).toContain("可投资资产 × 3% ÷ 年度预算总额");
    expect(popover!.textContent).toContain("安全提取率");
    touch.unmount();

    setFakeMedia({ hover: "hover", pointer: "fine" });
    const pointer = await mountView();
    await pointer.find('[data-testid="financial-freedom-info"]').trigger("click");
    await flushPromises();
    expect(document.body.querySelector(".n-popover")).toBeNull();
  });

  it("移动档：金额隐私模式生效（掩码替换各金额，栅格单列不回归）", async () => {
    setFakeMedia({ width: 600 });
    setCurrentMonthSummary({ income_cents: 100000, expense_cents: 80000, refund_cents: 5000 });
    const wrapper = await mountView();
    expect(wrapper.findAllComponents(NGrid)[0].props("cols")).toBe(1);
    const netWorthText = formatAmount(123456, cny);
    expect(wrapper.text()).toContain(netWorthText);
    amountPrivacyEnabled.value = true;
    await nextTick();
    const text = wrapper.text();
    expect(text).toContain("••••");
    expect(text).not.toContain(cny.symbol);
    expect(text).not.toContain(netWorthText);
    amountPrivacyEnabled.value = false;
    await nextTick();
    expect(wrapper.text()).toContain(netWorthText);
  });
});

describe("DashboardView 金额隐私模式（issue #567 仪表盘面核查：无 canvas 图表，数字卡全走格式化接缝）", () => {
  it("开启后各金额卡显示掩码、币种符号一同隐藏，百分比与件数保留", async () => {
    setCurrentMonthSummary({ income_cents: 100000, expense_cents: 80000, refund_cents: 5000 });
    const wrapper = await mountView();
    const netWorthText = formatAmount(123456, cny);
    expect(wrapper.text()).toContain(netWorthText);

    amountPrivacyEnabled.value = true;
    await nextTick();

    const text = wrapper.text();
    expect(text).toContain("••••");
    expect(text).not.toContain(cny.symbol);
    expect(text).not.toContain(netWorthText);
    expect(wrapper.find('[data-testid="financial-freedom-ratio"]').text()).toBe("7.5%");

    amountPrivacyEnabled.value = false;
    await nextTick();
    expect(wrapper.text()).toContain(netWorthText);
  });
});
