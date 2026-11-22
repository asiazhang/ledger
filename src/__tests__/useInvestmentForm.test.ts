import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { useReferenceStore } from "@/stores/reference";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import { useInvestmentForm } from "@/investment/useInvestmentForm";
import { makeAccount, makeGoalPair, makeInstrument } from "./factories";
import type {
  Account,
  Instrument,
  SavingsGoalProgress,
  Transaction,
  TransactionTrade,
} from "@ledger/types";

let goalProgress: SavingsGoalProgress[] = [];

const mockAccounts: Account[] = [
  {
    id: "acc-inv",
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
  {
    id: "acc-cash",
    name: "现金",
    type: "cash",
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

const mockFundInstruments: Instrument[] = [
  {
    id: "ins-fund",
    symbol: "000123",
    name: "某混合基金",
    type: "fund",
    currency_code: "CNY",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    market: "unknown",
    invested: false,
    source: "eastmoney",
    price_cents: null,
    price_channel: "fund_nav",
  },
];

const editingTx: Transaction = {
  id: "txn-buy-1",
  kind: "buy",
  amount_cents: 15500,
  currency_code: "CNY",
  amount_native_cents: 15500,
  fx_rate_used: null,
  fx_rate_source: null,
  source_order_no: null,
  account_id: "acc-inv",
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

const editingTrade: TransactionTrade = {
  instrument_id: "ins-1",
  symbol: "NVDA",
  instrument_name: "英伟达",
  instrument_type: "stock",
  quantity: 100,
  price_cents: 1500000,
  fee_cents: 500,
};

const editingFundTx: Transaction = {
  ...editingTx,
  id: "txn-fund-1",
  amount_cents: 100000,
};

const editingFundTrade: TransactionTrade = {
  instrument_id: "ins-fund",
  symbol: "000123",
  instrument_name: "某混合基金",
  instrument_type: "fund",
  quantity: 987.6543,
  price_cents: 10110,
  fee_cents: 150,
};

const BASE_OVERRIDES = { list_accounts: mockAccounts };

describe("useInvestmentForm", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    goalProgress = [];
    wireInvokeSeam({
      overrides: { ...BASE_OVERRIDES, savings_goal_progress: () => Promise.resolve(goalProgress) },
    });
  });

  it("初始化状态：账户/标的/数量/价格为空（数量/价格为原始文本，#416）", () => {
    const form = useInvestmentForm("buy");
    expect(form.accountId.value).toBeNull();
    expect(form.instrumentId.value).toBeNull();
    expect(form.quantityText.value).toBe("");
    expect(form.priceText.value).toBe("");
  });

  it("submit 校验：无账户/标的/数量/单价时警告且不写入", async () => {
    const form = useInvestmentForm("buy");
    await form.submit();
    form.accountId.value = "acc-inv";
    await form.submit();
    form.instrumentId.value = "ins-1";
    await form.submit();
    await form.submit();
    form.quantityText.value = "10";
    await form.submit();
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction")).toHaveLength(0);
  });

  it("submit 创建：调用 create_transaction，成功后重置表单", async () => {
    wireInvokeSeam({
      overrides: { ...BASE_OVERRIDES, create_transaction: Promise.resolve("new-txn") },
    });
    const onCreated = vi.fn();
    const form = useInvestmentForm("buy", { onCreated });
    form.accountId.value = "acc-inv";
    form.instrumentId.value = "ins-1";
    form.quantityText.value = "10";
    form.priceText.value = "15";
    form.fee.value = 5;
    form.note.value = "测试";
    form.date.value = new Date("2026-07-11").getTime();

    await form.submit();

    expect(mockInvoke).toHaveBeenCalledWith("create_transaction", {
      input: expect.objectContaining({ kind: "buy" }),
    });
    expect(form.instrumentId.value).toBeNull();
    expect(form.quantityText.value).toBe("");
    expect(form.priceText.value).toBe("");
    expect(form.quantityError.value).toBeNull();
    expect(onCreated).toHaveBeenCalledTimes(1);
  });

  describe("基金申赎形态（issue #302）：金额权威、单价反算", () => {
    async function searchFundCandidates(kind: "buy" | "sell") {
      vi.useFakeTimers();
      try {
        wireInvokeSeam({
          overrides: {
            ...BASE_OVERRIDES,
            list_instruments: Promise.resolve({ items: mockFundInstruments, total: 1 }),
          },
        });
        const form = useInvestmentForm(kind);
        form.searchInstruments("某混合");
        await vi.advanceTimersByTimeAsync(300);
        return form;
      } finally {
        vi.useRealTimers();
      }
    }

    it("选基金标的：isFundInstrument 打开，derivedPrice 按（金额 − 费用）× 100 ÷ 份额反算", async () => {
      const emptyForm = useInvestmentForm("buy");
      expect(emptyForm.isFundInstrument.value).toBe(false);
      expect(emptyForm.derivedPrice.value).toBeNull();
      const form = await searchFundCandidates("buy");
      form.instrumentId.value = "ins-fund";
      expect(form.isFundInstrument.value).toBe(true);
      form.amount.value = 1000;
      form.quantityText.value = "987.6543";
      form.fee.value = 1.5;
      expect(form.derivedPrice.value).toBeCloseTo(1.011, 6);
    });

    it("缺确认金额：警告且不写入（份额/单价校验不误伤）", async () => {
      const form = await searchFundCandidates("buy");
      form.instrumentId.value = "ins-fund";
      form.accountId.value = "acc-inv";
      form.quantityText.value = "987.6543";
      await form.submit();
      expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction")).toHaveLength(0);
    });

    it("submit 创建：确认单金额落 amount_cents、单价不落 wire（price_cents null）", async () => {
      const form = await searchFundCandidates("buy");
      wireInvokeSeam({
        overrides: { ...BASE_OVERRIDES, create_transaction: Promise.resolve("fund-txn") },
      });
      form.instrumentId.value = "ins-fund";
      form.accountId.value = "acc-inv";
      form.amount.value = 1000;
      form.quantityText.value = "987.6543";
      form.fee.value = 1.5;
      await form.submit();
      expect(mockInvoke).toHaveBeenCalledWith("create_transaction", {
        input: expect.objectContaining({
          kind: "buy",
          amount_cents: 100000,
          quantity: 987.6543,
          price_cents: null,
          fee_cents: 150,
        }),
      });
    });

    it("sell 反算口径：毛收入 = 金额 + 费用，derivedPrice 随之抬高", async () => {
      const form = await searchFundCandidates("sell");
      form.instrumentId.value = "ins-fund";
      form.amount.value = 520;
      form.quantityText.value = "500";
      form.fee.value = 0.52;
      expect(form.derivedPrice.value).toBeCloseTo(1.041, 6);
    });

    it("编辑回填：确认单金额回填 amount，单价不回填（由反算展示）", () => {
      const form = useInvestmentForm("buy", {
        editing: () => editingFundTx,
        trade: () => editingFundTrade,
      });
      expect(form.isFundInstrument.value).toBe(true);
      expect(form.amount.value).toBe(1000);
      expect(form.quantityText.value).toBe("987.6543");
      expect(form.fee.value).toBe(1.5);
      expect(form.priceText.value).toBe("");
      expect(form.priceError.value).toBeNull();
      expect(form.derivedPrice.value).toBeCloseTo(1.011, 6);
    });
  });

  describe("编辑模式（issue #180）", () => {
    it("创建即回填：账户/标的/数量/价格/费用/备注/日期/币种，标的候选项含回填标的", async () => {
      const store = useReferenceStore();
      await store.refresh();
      const form = useInvestmentForm("buy", {
        editing: () => editingTx,
        trade: () => editingTrade,
      });
      expect(form.accountId.value).toBe("acc-inv");
      expect(form.instrumentId.value).toBe("ins-1");
      expect(form.quantityText.value).toBe("100");
      expect(form.priceText.value).toBe("150");
      expect(form.quantityError.value).toBeNull();
      expect(form.priceError.value).toBeNull();
      expect(form.fee.value).toBe(5);
      expect(form.note.value).toBe("建仓买入");
      expect(form.date.value).toBe(new Date("2026-01-10T00:00:00Z").getTime());
      expect(form.currencyCode.value).toBe("CNY");
      expect(form.instrumentOptions.value.map((o) => o.value)).toEqual(["ins-1"]);
    });

    it("用户搜索不冲掉回填标的选项（回填钉住接线，删除 pin 即红）", async () => {
      const store = useReferenceStore();
      await store.refresh();
      const form = useInvestmentForm("buy", {
        editing: () => editingTx,
        trade: () => ({ ...editingTrade, instrument_name: null }),
      });
      wireInvokeSeam({
        overrides: {
          ...BASE_OVERRIDES,
          list_instruments: Promise.resolve({
            items: [makeInstrument({ id: "ins-2", symbol: "MU", name: "美光" })],
            total: 1,
          }),
        },
      });
      form.searchInstruments("美光");
      await vi.waitFor(() => {
        expect(form.instrumentOptions.value.map((o) => o.value)).toContain("ins-2");
      });
      expect(form.instrumentOptions.value.map((o) => o.value)).toEqual(["ins-1", "ins-2"]);
    });

    it("submit 编辑：分派 update_transaction（同一入参形状），onUpdated 触发、onCreated 不触发、不重置表单", async () => {
      const store = useReferenceStore();
      await store.refresh();
      wireInvokeSeam({
        overrides: { ...BASE_OVERRIDES, update_transaction: Promise.resolve(null) },
      });
      const onCreated = vi.fn();
      const onUpdated = vi.fn();
      const form = useInvestmentForm("buy", {
        onCreated,
        onUpdated,
        editing: () => editingTx,
        trade: () => editingTrade,
      });

      await form.submit();

      expect(mockInvoke).toHaveBeenCalledWith("update_transaction", {
        id: "txn-buy-1",
        input: expect.objectContaining({
          kind: "buy",
          instrument_id: "ins-1",
          quantity: 100,
          note: "建仓买入",
        }),
      });
      expect(onUpdated).toHaveBeenCalledTimes(1);
      expect(onCreated).not.toHaveBeenCalled();
      expect(form.instrumentId.value).toBe("ins-1");
      expect(form.quantityText.value).toBe("100");
    });

    it("submit 编辑失败：错误不抛出、onUpdated 不触发、已填内容不丢", async () => {
      const store = useReferenceStore();
      await store.refresh();
      const onUpdated = vi.fn();
      const form = useInvestmentForm("buy", {
        onUpdated,
        editing: () => editingTx,
        trade: () => editingTrade,
      });
      wireInvokeSeam({
        overrides: {
          ...BASE_OVERRIDES,
          update_transaction: Promise.reject(new Error("该买入交易已有部分卖出，无法修改")),
        },
      });

      await expect(form.submit()).resolves.toBeUndefined();
      expect(onUpdated).not.toHaveBeenCalled();
      expect(form.instrumentId.value).toBe("ins-1");
      expect(form.quantityText.value).toBe("100");
      expect(form.priceText.value).toBe("150");
    });
  });
});

function pair(id: string) {
  return makeGoalPair({ id, goal: { id: `goal-${id}` } });
}

describe("useInvestmentForm 出资账户（issue #936 / #938 / ADR-0096，buy/sell 对称）", () => {
  const fundingAccounts: Account[] = [
    {
      id: "acc-cash",
      name: "现金钱包",
      type: "cash",
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
    {
      id: "acc-credit",
      name: "信用卡",
      type: "credit",
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
    {
      id: "acc-ewallet",
      name: "零钱通",
      type: "ewallet",
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
    {
      id: "acc-other",
      name: "其他现金",
      type: "other",
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
    makeGoalPair({ id: "acc-goal", account: { name: "买车基金账户" } }).account,
    makeGoalPair({
      id: "acc-goal-usd",
      account: { name: "美元目标账户", currency_code: "USD" },
    }).account,
    {
      id: "acc-inv",
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
    makeAccount({ id: "acc-inv-usd", name: "美股证券户", currency_code: "USD" }),
    {
      id: "acc-recv",
      name: "借出·张三",
      type: "receivable",
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
    {
      id: "acc-debt",
      name: "借入·李四",
      type: "debt",
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
    {
      id: "acc-bank-usd",
      name: "美元卡",
      type: "bank",
      currency_code: "USD",
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

  async function fundingForm(
    options?: Parameters<typeof useInvestmentForm>[1],
    kind: "buy" | "sell" = "buy",
  ) {
    wireInvokeSeam({
      overrides: {
        list_accounts: fundingAccounts,
        savings_goal_progress: () => Promise.resolve(goalProgress),
      },
    });
    const store = useReferenceStore();
    await store.refresh();
    await useSavingsGoalsStore()
      .refresh()
      .catch(() => {});
    return useInvestmentForm(kind, options);
  }

  async function fundingCandidates(expectGoalFiltered: boolean, kind: "buy" | "sell" = "buy") {
    const form = await fundingForm(undefined, kind);
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === "savings_goal_progress"),
      "候选过滤消费目标绑定集：进度命令被拉取（调用事实）",
    ).toBe(true);
    const values = form.fundingAccountOptions.value.map((o) => o.value);
    if (expectGoalFiltered) {
      expect(values, "在用目标账户不在出资候选").not.toContain("acc-goal");
    } else {
      expect(values, "无绑定的普通 other 账户不被误伤").toContain("acc-other");
    }
    return values;
  }

  it("候选过滤：含在用目标账户不在候选（issue #1755 / ADR-0133 决策 4）", async () => {
    goalProgress = [pair("acc-goal").progress];
    expect(await fundingCandidates(true)).toEqual([
      "acc-cash",
      "acc-bank",
      "acc-credit",
      "acc-ewallet",
      "acc-other",
    ]);
  });

  it("无绑定不误伤：普通 other 账户仍在候选；默认空（issue #1755）", async () => {
    const values = await fundingCandidates(false);
    expect(values).toEqual(["acc-cash", "acc-bank", "acc-credit", "acc-ewallet", "acc-other"]);
  });

  it("币种随投资账户联动：选美元投资账户后候选只剩同币种现金类，同币种目标账户同样被滤（issue #1191 / #1755）", async () => {
    goalProgress = [pair("acc-goal-usd").progress];
    const form = await fundingForm();
    expect(form.currencyCode.value).toBe("CNY");
    form.accountId.value = "acc-inv-usd";
    expect(form.currencyCode.value).toBe("USD");
    expect(form.fundingAccountOptions.value.map((o) => o.value)).toEqual(["acc-bank-usd"]);
  });

  it("编辑回填：带出资账户的买入带出当前值，历史买入不带（保持空）", async () => {
    const withFunding = await fundingForm({
      editing: () => ({ ...editingTx, funding_account_id: "acc-bank" }),
      trade: () => editingTrade,
    });
    expect(withFunding.fundingAccountId.value).toBe("acc-bank");
    const withoutFunding = await fundingForm({
      editing: () => editingTx,
      trade: () => editingTrade,
    });
    expect(withoutFunding.fundingAccountId.value).toBeNull();
  });

  it("submit 创建：出资账户随装配落 funding_account_id；重置表单后回空", async () => {
    wireInvokeSeam({
      overrides: { list_accounts: fundingAccounts, create_transaction: Promise.resolve("new-txn") },
    });
    const store = useReferenceStore();
    await store.refresh();
    const onCreated = vi.fn();
    const form = useInvestmentForm("buy", { onCreated });
    form.accountId.value = "acc-inv";
    form.instrumentId.value = "ins-1";
    form.quantityText.value = "10";
    form.priceText.value = "15";
    form.fundingAccountId.value = "acc-bank";
    await form.submit();
    expect(mockInvoke).toHaveBeenCalledWith("create_transaction", {
      input: expect.objectContaining({ kind: "buy", funding_account_id: "acc-bank" }),
    });
    expect(onCreated).toHaveBeenCalledTimes(1);
  });

  it("submit 编辑不改出资账户：回填值随全字段替换原样提交（字段不被静默丢失）", async () => {
    const form = await fundingForm({
      onUpdated: vi.fn(),
      editing: () => ({ ...editingTx, funding_account_id: "acc-bank" }),
      trade: () => editingTrade,
    });
    wireInvokeSeam({
      overrides: { list_accounts: fundingAccounts, update_transaction: Promise.resolve(null) },
    });
    await form.submit();
    expect(mockInvoke).toHaveBeenCalledWith("update_transaction", {
      id: "txn-buy-1",
      input: expect.objectContaining({ funding_account_id: "acc-bank" }),
    });
  });

  it("submit 编辑清空出资账户：提交显式 null（改/清语义正确，非缺字段）", async () => {
    const form = await fundingForm({
      onUpdated: vi.fn(),
      editing: () => ({ ...editingTx, funding_account_id: "acc-bank" }),
      trade: () => editingTrade,
    });
    wireInvokeSeam({
      overrides: { list_accounts: fundingAccounts, update_transaction: Promise.resolve(null) },
    });
    form.fundingAccountId.value = null;
    await form.submit();
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "update_transaction")!;
    expect(
      (call[1] as { input: { funding_account_id: string | null } }).input.funding_account_id,
    ).toBeNull();
  });

  it("sell 候选过滤同款：在用目标账户不在候选、默认空（issue #1755 对称）", async () => {
    goalProgress = [pair("acc-goal").progress, pair("acc-goal-usd").progress];
    const form = await fundingForm(undefined, "sell");
    expect(form.fundingAccountId.value).toBeNull();
    expect(form.fundingAccountOptions.value.map((o) => o.value)).toEqual([
      "acc-cash",
      "acc-bank",
      "acc-credit",
      "acc-ewallet",
      "acc-other",
    ]);
    form.accountId.value = "acc-inv-usd";
    expect(form.currencyCode.value).toBe("USD");
    expect(form.fundingAccountOptions.value.map((o) => o.value)).toEqual(["acc-bank-usd"]);
  });

  it("sell submit 创建：出资账户随装配落 funding_account_id（kind sell）", async () => {
    wireInvokeSeam({
      overrides: { list_accounts: fundingAccounts, create_transaction: Promise.resolve("new-txn") },
    });
    const store = useReferenceStore();
    await store.refresh();
    const form = useInvestmentForm("sell");
    form.accountId.value = "acc-inv";
    form.instrumentId.value = "ins-1";
    form.quantityText.value = "10";
    form.priceText.value = "15";
    form.fundingAccountId.value = "acc-bank";
    await form.submit();
    expect(mockInvoke).toHaveBeenCalledWith("create_transaction", {
      input: expect.objectContaining({ kind: "sell", funding_account_id: "acc-bank" }),
    });
  });

  it("sell 编辑回填同款：带出资账户带出当前值，不带保持空", async () => {
    const editingSellTx = { ...editingTx, kind: "sell" as const, id: "txn-sell-1" };
    const withFunding = await fundingForm(
      {
        editing: () => ({ ...editingSellTx, funding_account_id: "acc-bank" }),
        trade: () => editingTrade,
      },
      "sell",
    );
    expect(withFunding.fundingAccountId.value).toBe("acc-bank");
    const withoutFunding = await fundingForm(
      { editing: () => editingSellTx, trade: () => editingTrade },
      "sell",
    );
    expect(withoutFunding.fundingAccountId.value).toBeNull();
  });

  it("sell submit 编辑清空出资账户：提交显式 null（回卡改余额卖出）", async () => {
    const editingSellTx = { ...editingTx, kind: "sell" as const, id: "txn-sell-1" };
    const form = await fundingForm(
      {
        onUpdated: vi.fn(),
        editing: () => ({ ...editingSellTx, funding_account_id: "acc-bank" }),
        trade: () => editingTrade,
      },
      "sell",
    );
    wireInvokeSeam({
      overrides: { list_accounts: fundingAccounts, update_transaction: Promise.resolve(null) },
    });
    form.fundingAccountId.value = null;
    await form.submit();
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "update_transaction")!;
    expect(
      (call[1] as { input: { funding_account_id: string | null } }).input.funding_account_id,
    ).toBeNull();
  });
});
