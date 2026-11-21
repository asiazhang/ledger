import { describe, it, expect, vi, beforeEach, beforeAll } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import {
  mockInvoke,
  wireInvokeSeam,
  type InvokeSeamOverride,
} from "@ledger/test-support/invoke-mock";
import { findButton } from "@ledger/test-support/dom";
import { mount, flushPromises } from "@vue/test-utils";
import { NSelect } from "naive-ui";
import { useReferenceStore } from "@/stores/reference";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import InvestmentForm from "@/investment/InvestmentForm.vue";
import { makeAccount, makeGoalPair } from "./factories";
import type { Account, Instrument, SavingsGoalProgress } from "@ledger/types";

beforeAll(() => {
  Element.prototype.scrollTo = () => {};
});

const mockAccounts: Account[] = [
  {
    id: "acc-1",
    name: "证券户",
    type: "investment",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    is_hidden: false,
    is_archived: false,
  },
];

const mockInstruments: Instrument[] = [
  {
    id: "ins-1",
    symbol: "NVDA",
    name: "英伟达",
    type: "stock",
    currency_code: "CNY",
    market: "unknown",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    source: "eastmoney",
    price_cents: null,
    invested: false,
    price_channel: "none",
  },
];

function instrumentSelect(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAllComponents(NSelect).find((s) => s.props("remote"))!;
}

const BASE_OVERRIDES = {
  list_accounts: mockAccounts,
  list_instruments: { items: [], total: 0 },
};

let base: ReturnType<typeof wireInvokeSeam>;

let goalProgress: SavingsGoalProgress[] = [];

beforeEach(async () => {
  setActivePinia(createPinia());
  goalProgress = [];
  base = wireInvokeSeam({
    overrides: { ...BASE_OVERRIDES, savings_goal_progress: () => Promise.resolve(goalProgress) },
  });
  const store = useReferenceStore();
  await store.refresh();
});

describe("InvestmentForm.vue 移除「新增标的」入口（issue #152）", () => {
  it("不渲染「新增标的」切换按钮与内嵌建档表单", () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    const newBtn = findButton(wrapper, "新增标的");
    expect(newBtn).toBeUndefined();
    expect(wrapper.text()).not.toContain("保存标的");
    expect(wrapper.text()).not.toContain("新增标的");
  });

  it("标的无候选时下拉空态提示「未找到标的，可通过同步或 AI 导入新增」", async () => {
    const wrapper = mount(InvestmentForm, {
      props: { kind: "buy", submitLabel: "记买入" },
      attachTo: document.body,
    });
    await instrumentSelect(wrapper).find(".n-base-selection").trigger("click");
    await flushPromises();
    expect(document.body.textContent).toContain("未找到标的，可通过同步或 AI 导入新增");
  });

  it("标的搜索与选择行为不受影响：搜索触发 list_instruments、候选可选择", async () => {
    vi.useFakeTimers();
    try {
      const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
      const select = instrumentSelect(wrapper);
      await select.find("input").setValue("NVDA");
      await vi.advanceTimersByTimeAsync(300);
      const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_instruments");
      expect(calls).toHaveLength(1);
      const [, args] = calls[0] as [string, { filter: { search: string } }];
      expect(args.filter.search).toBe("NVDA");
      wireInvokeSeam({
        overrides: {
          ...BASE_OVERRIDES,
          list_instruments: Promise.resolve({ items: mockInstruments, total: 1 }),
        },
      });
      await select.find("input").setValue("NVDA");
      await vi.advanceTimersByTimeAsync(300);
      await flushPromises();
      expect((select.props("options") as { value: string }[]).map((o) => o.value)).toEqual([
        "ins-1",
      ]);
      select.vm.$emit("update:value", "ins-1");
      await flushPromises();
      expect(select.props("value")).toBe("ins-1");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("InvestmentForm.vue 基金申赎形态（issue #302）", () => {
  const fundInstruments = [
    {
      id: "ins-fund",
      symbol: "000123",
      name: "某混合基金",
      type: "fund" as const,
      currency_code: "CNY",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      version: 1,
      device_id: "test",
      is_deleted: false,
      market: "unknown" as const,
      invested: false,
      price_cents: null,
    },
  ];

  async function mountWithFundSelected(kind: "buy" | "sell") {
    const wrapper = mount(InvestmentForm, {
      props: { kind, submitLabel: kind === "buy" ? "记买入" : "记卖出" },
    });
    const select = instrumentSelect(wrapper);
    vi.useFakeTimers();
    try {
      await select.find("input").setValue("某混合");
      await vi.advanceTimersByTimeAsync(300);
      await flushPromises();
    } finally {
      vi.useRealTimers();
    }
    select.vm.$emit("update:value", "ins-fund");
    await flushPromises();
    return wrapper;
  }

  function inputByPlaceholder(wrapper: ReturnType<typeof mount>, placeholder: string) {
    return wrapper.findAll("input").find((i) => i.attributes("placeholder") === placeholder);
  }

  it("选基金标的：金额可编辑（确认单权威）、份额标签、单价只读反算", async () => {
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        list_instruments: Promise.resolve({ items: fundInstruments, total: 1 }),
      },
    });
    const wrapper = await mountWithFundSelected("buy");
    expect(inputByPlaceholder(wrapper, "确认金额（以确认单为准）")).toBeDefined();
    expect(inputByPlaceholder(wrapper, "确认份额（以确认单为准）")).toBeDefined();
    const priceInput = inputByPlaceholder(wrapper, "由金额与份额反算");
    expect(priceInput).toBeDefined();
    expect(priceInput!.attributes("disabled")).toBeDefined();
    expect(inputByPlaceholder(wrapper, "自动计算")).toBeUndefined();
  });

  it("未选标的时保持股票形态（金额只读展示、单价可编辑）", () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    expect(inputByPlaceholder(wrapper, "自动计算")).toBeDefined();
    expect(inputByPlaceholder(wrapper, "确认金额（以确认单为准）")).toBeUndefined();
    const priceInput = inputByPlaceholder(wrapper, "单价");
    expect(priceInput).toBeDefined();
    expect(priceInput!.attributes("disabled")).toBeUndefined();
  });
});

describe("InvestmentForm.vue 字段错误态（ADR-0058 / issue #416）", () => {
  function quantityInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll("input").find((i) => i.attributes("placeholder") === "数量")!;
  }

  function priceInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll("input").find((i) => i.attributes("placeholder") === "单价")!;
  }

  function inputRoot(input: ReturnType<typeof quantityInput>) {
    const el = input.element.closest(".n-input");
    expect(el).not.toBeNull();
    return el as Element;
  }

  function hasErrorStatus(input: ReturnType<typeof quantityInput>) {
    return inputRoot(input).classList.contains("n-input--error-status");
  }

  function submitButton(wrapper: ReturnType<typeof mount>, label: string) {
    return findButton(wrapper, label)!;
  }

  function selectByIndex(wrapper: ReturnType<typeof mount>, index: number, value: string) {
    return wrapper.findAllComponents(NSelect)[index].vm.$emit("update:value", value);
  }

  function createCalls() {
    return mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction");
  }

  it("初始为空不红，保存按钮可点（不惩罚尚未输入）", () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false);
    expect(hasErrorStatus(priceInput(wrapper))).toBe(false);
    expect(submitButton(wrapper, "记买入").attributes("disabled")).toBeUndefined();
  });

  it("数量输入解析失败文本（4.30发）即时红显、保存禁用、非法文本原样保留", async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    await quantityInput(wrapper).setValue("4.30发");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true);
    expect(submitButton(wrapper, "记买入").attributes("disabled")).toBeDefined();
    expect((quantityInput(wrapper).element as HTMLInputElement).value).toBe("4.30发");
  });

  it("数量超四位小数（1.23456）即时红显", async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    await quantityInput(wrapper).setValue("1.23456");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true);
  });

  it("单价超四位小数（1.23456）即时红显、保存禁用；四位小数（1.2345）合法不红", async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    await priceInput(wrapper).setValue("1.23456");
    expect(hasErrorStatus(priceInput(wrapper))).toBe(true);
    expect(submitButton(wrapper, "记买入").attributes("disabled")).toBeDefined();
    await priceInput(wrapper).setValue("1.2345");
    expect(hasErrorStatus(priceInput(wrapper))).toBe(false);
    expect(submitButton(wrapper, "记买入").attributes("disabled")).toBeUndefined();
  });

  it("非法文本失焦不清空、红态持续；修正后红态解除、保存恢复可点", async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    await quantityInput(wrapper).setValue("4.30发");
    await quantityInput(wrapper).trigger("blur");
    expect((quantityInput(wrapper).element as HTMLInputElement).value).toBe("4.30发");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true);
    await quantityInput(wrapper).setValue("100");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false);
    expect(submitButton(wrapper, "记买入").attributes("disabled")).toBeUndefined();
  });

  it("清空后未失焦不红；失焦红；重新输入合法解除", async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    await quantityInput(wrapper).setValue("12");
    await quantityInput(wrapper).setValue("");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false);
    await quantityInput(wrapper).trigger("blur");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true);
    await quantityInput(wrapper).setValue("12");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false);
  });

  it("保存尝试时空值红显兜底（数量+单价同红），不发起提交（格式类 toast 被红态取代）", async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    await submitButton(wrapper, "记买入").trigger("click");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(true);
    expect(hasErrorStatus(priceInput(wrapper))).toBe(true);
    expect(createCalls()).toHaveLength(0);
  });

  it("编辑弹窗合法回填（数量 100 / 单价 150）不显示红态、保存可点", () => {
    const editingTx = {
      id: "txn-buy-1",
      kind: "buy" as const,
      amount_cents: 15500,
      currency_code: "CNY",
      amount_native_cents: 15500,
      fx_rate_used: null,
      fx_rate_source: null,
      source_order_no: null,
      account_id: "acc-1",
      to_account_id: null,
      funding_account_id: null,
      fundings: [],
      purchases: [],
      category_id: null,
      merchant_id: null,
      policy_id: null,
      source: null,
      convert: null,
      refund_of_transaction_id: null,
      note: null,
      date: "2026-01-10",
      created_at: "2026-01-10T01:00:00Z",
      updated_at: "2026-01-10T00:00:00Z",
      version: 1,
      device_id: "test",
      is_deleted: false,
    };
    const editingTrade = {
      instrument_id: "ins-1",
      symbol: "NVDA",
      instrument_name: "英伟达",
      instrument_type: "stock" as const,
      quantity: 100,
      price_cents: 1500000,
      fee_cents: 500,
    };
    const wrapper = mount(InvestmentForm, {
      props: { kind: "buy", submitLabel: "记买入", editing: editingTx, trade: editingTrade },
    });
    expect((quantityInput(wrapper).element as HTMLInputElement).value).toBe("100");
    expect((priceInput(wrapper).element as HTMLInputElement).value).toBe("150");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false);
    expect(hasErrorStatus(priceInput(wrapper))).toBe(false);
    expect(submitButton(wrapper, "保存修改").attributes("disabled")).toBeUndefined();
  });

  it("纯零/负数数量不红、保存可点、提交走业务类校验通道（不发起 create_transaction）", async () => {
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    await selectByIndex(wrapper, 1, "acc-1");
    await selectByIndex(wrapper, 2, "ins-1");
    await quantityInput(wrapper).setValue("0");
    await priceInput(wrapper).setValue("10");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false);
    expect(submitButton(wrapper, "记买入").attributes("disabled")).toBeUndefined();
    await submitButton(wrapper, "记买入").trigger("click");
    await flushPromises();
    expect(createCalls()).toHaveLength(0);
    await quantityInput(wrapper).setValue("-5");
    expect(hasErrorStatus(quantityInput(wrapper))).toBe(false);
    await submitButton(wrapper, "记买入").trigger("click");
    await flushPromises();
    expect(createCalls()).toHaveLength(0);
  });

  it("基金形态：份额字段红态同规、单价为只读反算无输入面、修正后保存恢复", async () => {
    const fundInstruments = [
      {
        id: "ins-fund",
        symbol: "000123",
        name: "某混合基金",
        type: "fund" as const,
        currency_code: "CNY",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        version: 1,
        device_id: "test",
        is_deleted: false,
        market: "unknown" as const,
        invested: false,
        price_cents: null,
      },
    ];
    wireInvokeSeam({
      overrides: {
        ...BASE_OVERRIDES,
        list_instruments: Promise.resolve({ items: fundInstruments, total: 1 }),
      },
    });
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    const select = instrumentSelect(wrapper);
    vi.useFakeTimers();
    try {
      await select.find("input").setValue("某混合");
      await vi.advanceTimersByTimeAsync(300);
      await flushPromises();
    } finally {
      vi.useRealTimers();
    }
    select.vm.$emit("update:value", "ins-fund");
    await flushPromises();

    const sharesInput = wrapper
      .findAll("input")
      .find((i) => i.attributes("placeholder") === "确认份额（以确认单为准）")!;
    await sharesInput.setValue("4.30发");
    expect(hasErrorStatus(sharesInput)).toBe(true);
    expect(submitButton(wrapper, "记买入").attributes("disabled")).toBeDefined();
    const derivedInput = wrapper
      .findAll("input")
      .find((i) => i.attributes("placeholder") === "由金额与份额反算")!;
    expect(derivedInput.attributes("disabled")).toBeDefined();
    await sharesInput.setValue("987.6543");
    expect(hasErrorStatus(sharesInput)).toBe(false);
    expect(submitButton(wrapper, "记买入").attributes("disabled")).toBeUndefined();
  });
});

describe("InvestmentForm.vue 编辑模式（issue #180）", () => {
  const editingTx = {
    id: "txn-buy-1",
    kind: "buy" as const,
    amount_cents: 15500,
    currency_code: "CNY",
    amount_native_cents: 15500,
    fx_rate_used: null,
    fx_rate_source: null,
    source_order_no: null,
    account_id: "acc-1",
    to_account_id: null,
    funding_account_id: null,
    fundings: [],
    purchases: [],
    category_id: null,
    merchant_id: null,
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    note: "建仓买入",
    date: "2026-01-10",
    created_at: "2026-01-10T01:00:00Z",
    updated_at: "2026-01-10T01:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  };

  const editingTrade = {
    instrument_id: "ins-1",
    symbol: "NVDA",
    instrument_name: "英伟达",
    instrument_type: "stock" as const,
    quantity: 100,
    price_cents: 15000,
    fee_cents: 500,
  };

  it("编辑回填：标的候选直接显示 symbol · name（不依赖搜索），按钮文案「保存修改」", () => {
    const wrapper = mount(InvestmentForm, {
      props: { kind: "buy", submitLabel: "记买入", editing: editingTx, trade: editingTrade },
    });
    const select = instrumentSelect(wrapper);
    expect(select.props("value")).toBe("ins-1");
    expect((select.props("options") as Array<{ label: string }>)[0].label).toBe("NVDA · 英伟达");
    expect(wrapper.text()).toContain("保存修改");
    expect(wrapper.text()).not.toContain("记买入");
  });

  it("编辑提交：触发 saved 事件（父层据此关窗），创建路径仍触发 created", async () => {
    const wrapper = mount(InvestmentForm, {
      props: { kind: "buy", submitLabel: "记买入", editing: editingTx, trade: editingTrade },
    });
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === "update_transaction" ? Promise.resolve() : base(cmd, args),
    );
    await findButton(wrapper, "保存修改")!.trigger("click");
    await flushPromises();
    expect(wrapper.emitted("saved")).toHaveLength(1);
    expect(wrapper.emitted("created")).toBeUndefined();
  });
});

describe("出资账户表单行（issue #936 / #938 / ADR-0096，buy/sell 对称）", () => {
  const accountsWithFunding = [
    ...mockAccounts,
    {
      id: "acc-bank",
      name: "招商银行卡",
      type: "bank",
      currency_code: "CNY",
      initial_balance_cents: 0,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      version: 1,
      device_id: "test",
      is_deleted: false,
      is_hidden: false,
      is_archived: false,
    },
    makeAccount({ id: "acc-inv-usd", name: "美股证券户", currency_code: "USD" }),
    makeAccount({ id: "acc-bank-usd", name: "美元卡", type: "bank", currency_code: "USD" }),
    makeGoalPair({ id: "acc-goal", account: { name: "买车基金账户" } }).account,
    makeGoalPair({
      id: "acc-goal-usd",
      account: { name: "美元目标账户", currency_code: "USD" },
    }).account,
  ];
  const editingTx = {
    id: "txn-buy-1",
    kind: "buy" as const,
    amount_cents: 15500,
    currency_code: "CNY",
    amount_native_cents: 15500,
    fx_rate_used: null,
    fx_rate_source: null,
    source_order_no: null,
    account_id: "acc-1",
    to_account_id: null,
    funding_account_id: "acc-bank" as string | null,
    fundings: [],
    purchases: [],
    category_id: null,
    merchant_id: null,
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    note: null,
    date: "2026-01-10",
    created_at: "2026-01-10T01:00:00Z",
    updated_at: "2026-01-10T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  };
  const editingTrade = {
    instrument_id: "ins-1",
    symbol: "NVDA",
    instrument_name: "英伟达",
    instrument_type: "stock" as const,
    quantity: 100,
    price_cents: 1500000,
    fee_cents: 500,
  };

  function fundingSelect(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAllComponents(NSelect)
      .find((s) =>
        (s.props("options") as Array<{ value: string }>).some((o) => o.value === "acc-bank"),
      )!;
  }

  const currencyDict = [
    { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 },
    { code: "USD", name: "美元", symbol: "$", decimal_places: 2 },
  ];

  function selectByTestId(wrapper: ReturnType<typeof mount>, testid: string) {
    return wrapper.findAllComponents(NSelect).find((s) => s.attributes("data-testid") === testid)!;
  }

  function fundingCandidates(wrapper: ReturnType<typeof mount>) {
    return (
      selectByTestId(wrapper, "investment-funding-account").props("options") as Array<{
        label: string;
        value: string;
      }>
    ).map((o) => `${o.label}|${o.value}`);
  }

  async function mountWithFundingReference(overrides: Record<string, InvokeSeamOverride> = {}) {
    wireInvokeSeam({
      overrides: {
        list_accounts: accountsWithFunding,
        list_instruments: { items: [], total: 0 },
        savings_goal_progress: () => Promise.resolve(goalProgress),
        ...overrides,
      },
    });
    await useReferenceStore().refresh();
    await useSavingsGoalsStore()
      .refresh()
      .catch(() => {});
  }

  it("买入与卖出都渲染出资账户下拉（默认空、可选清空，#938 卖出对称落地）", () => {
    const buy = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    expect(buy.text()).toContain("出资账户");
    const sell = mount(InvestmentForm, { props: { kind: "sell", submitLabel: "记卖出" } });
    expect(sell.text()).toContain("出资账户");
  });

  it("编辑回填：带出资账户的买入打开即显示当前值（AC「字段回填显示」）", async () => {
    await mountWithFundingReference();
    const wrapper = mount(InvestmentForm, {
      props: { kind: "buy", submitLabel: "记买入", editing: editingTx, trade: editingTrade },
    });
    expect(fundingSelect(wrapper).props("value")).toBe("acc-bank");
    const untouched = mount(InvestmentForm, {
      props: {
        kind: "buy",
        submitLabel: "记买入",
        editing: { ...editingTx, funding_account_id: null },
        trade: editingTrade,
      },
    });
    expect(fundingSelect(untouched).props("value")).toBeNull();
  });

  it("外币投资账户（新建）：币种随账户、候选含同币种现金账户、同币种目标户不在候选（issue #1191 / #1755）", async () => {
    goalProgress = [
      makeGoalPair({ id: "acc-goal" }).progress,
      makeGoalPair({ id: "acc-goal-usd" }).progress,
    ];
    await mountWithFundingReference({ list_currencies: currencyDict });
    const wrapper = mount(InvestmentForm, { props: { kind: "buy", submitLabel: "记买入" } });
    expect(selectByTestId(wrapper, "investment-currency").text()).toContain("人民币 (CNY)");
    expect(fundingCandidates(wrapper)).toEqual(["招商银行卡|acc-bank"]);
    selectByTestId(wrapper, "investment-account").vm.$emit("update:value", "acc-inv-usd");
    await flushPromises();
    expect(selectByTestId(wrapper, "investment-currency").text()).toContain("美元 (USD)");
    expect(fundingCandidates(wrapper)).toEqual(["美元卡|acc-bank-usd"]);
  });

  it("外币投资账户（编辑回填）：币种随账户、候选含同币种现金账户、同币种目标户不在候选（issue #1191 / #1755）", async () => {
    goalProgress = [
      makeGoalPair({ id: "acc-goal" }).progress,
      makeGoalPair({ id: "acc-goal-usd" }).progress,
    ];
    await mountWithFundingReference({ list_currencies: currencyDict });
    const wrapper = mount(InvestmentForm, {
      props: {
        kind: "buy",
        submitLabel: "记买入",
        editing: {
          ...editingTx,
          id: "txn-buy-usd",
          account_id: "acc-inv-usd",
          currency_code: "USD",
          funding_account_id: "acc-bank-usd",
        },
        trade: editingTrade,
      },
    });
    expect(selectByTestId(wrapper, "investment-currency").text()).toContain("美元 (USD)");
    expect(selectByTestId(wrapper, "investment-funding-account").props("value")).toBe(
      "acc-bank-usd",
    );
    expect(fundingCandidates(wrapper)).toEqual(["美元卡|acc-bank-usd"]);
  });

  it("卖出编辑回填与买入同款：带出资账户打开即显示当前值（#938 对称）", async () => {
    await mountWithFundingReference();
    const wrapper = mount(InvestmentForm, {
      props: {
        kind: "sell",
        submitLabel: "记卖出",
        editing: { ...editingTx, kind: "sell", id: "txn-sell-1" },
        trade: editingTrade,
      },
    });
    expect(fundingSelect(wrapper).props("value")).toBe("acc-bank");
  });
});
