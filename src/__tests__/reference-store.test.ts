import { describe, it, expect, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { useReferenceStore } from "@/stores/reference";
import { makeAccount } from "./factories";
import type { Account, Category, Currency, Insurer, Merchant } from "@ledger/types";

const mockCurrencies: Currency[] = [
  { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 },
  { code: "USD", name: "美元", symbol: "$", decimal_places: 2 },
];

const mockAccounts: Account[] = [
  {
    id: "acc-1",
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
  {
    id: "acc-2",
    name: "招商银行",
    type: "bank",
    currency_code: "CNY",
    initial_balance_cents: 100000,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    is_hidden: false,
    is_archived: false,
  },
];

const mockCategories: Category[] = [
  {
    id: "cat-root",
    name: "餐饮",
    kind: "expense",
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
  {
    id: "cat-child",
    name: "外卖",
    kind: "expense",
    parent_id: "cat-root",
    icon: null,
    sort_order: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
  {
    id: "cat-income",
    name: "工资",
    kind: "income",
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
  {
    id: "cat-del",
    name: "旅游",
    kind: "expense",
    parent_id: "cat-root",
    icon: null,
    sort_order: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: true,
  },
];

const mockMerchants: Merchant[] = [
  {
    id: "mch-1",
    name: "京东",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
  {
    id: "mch-2",
    name: "红旗连锁",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
];

const mockInsurers: Insurer[] = [
  {
    id: "ins-1",
    name: "平安人寿",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
  {
    id: "ins-del",
    name: "已删保司",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: true,
  },
];

function mockListCommands() {
  wireInvokeSeam({
    overrides: {
      list_currencies: mockCurrencies,
      list_accounts: mockAccounts,
      list_categories: mockCategories,
      list_merchants: mockMerchants,
      list_insurers: mockInsurers,
    },
  });
}

beforeEach(() => {
  mockListCommands();
});

describe("useReferenceStore", () => {
  it("初始状态为空", () => {
    const store = useReferenceStore();
    expect(store.currencies).toEqual([]);
    expect(store.accounts).toEqual([]);
    expect(store.categories).toEqual([]);
    expect(store.merchants).toEqual([]);
    expect(store.insurers).toEqual([]);
    expect(store.deletedInsurers.size).toBe(0);
  });

  it("refresh 拉取五张参考表并填充响应式状态（保司 issue #714）", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.currencies).toEqual(mockCurrencies);
    expect(store.accounts).toEqual(mockAccounts);
    expect(store.categories.map((c) => c.id)).toEqual(["cat-root", "cat-child", "cat-income"]);
    expect(store.merchants).toEqual(mockMerchants);
    expect(store.insurers.map((i) => i.id)).toEqual(["ins-1"]);
    expect(store.deletedInsurers.get("ins-del")?.name).toBe("已删保司");
  });

  it("list_insurers 以含已删全量拉取（includeDeleted=true，管理视图「显示已删」数据源 issue #714）", async () => {
    const store = useReferenceStore();
    await store.refresh();
    const insurerCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_insurers");
    expect(insurerCalls.length).toBeGreaterThan(0);
    for (const [, args] of insurerCalls) {
      expect(args).toMatchObject({ includeDeleted: true });
    }
  });

  it("软删保司：从在用字典消失（不可再选），deletedInsurers 保留（管理视图已删区 issue #714）", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.insurers.map((i) => i.id)).toEqual(["ins-1"]);

    wireInvokeSeam({
      overrides: {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: mockCategories,
        list_merchants: mockMerchants,
        list_insurers: [{ ...mockInsurers[0], is_deleted: true }, mockInsurers[1]],
      },
    });
    await store.refresh();

    expect(store.insurers).toEqual([]);
    expect(store.deletedInsurers.get("ins-1")?.name).toBe("平安人寿");
    expect(store.deletedInsurers.get("ins-del")?.name).toBe("已删保司");
  });

  it("派生映射 currencyMap/accountMap/categoryMap 正确", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.currencyMap.get("USD")?.name).toBe("美元");
    expect(store.accountMap.get("acc-2")?.name).toBe("招商银行");
    expect(store.categoryMap.get("cat-child")?.name).toBe("外卖");
  });

  it("投资账户下拉候选面（issue #1830）：谓词→{label,value} 投影单点——非投资类不进、隐藏保留、label=账户名", async () => {
    wireInvokeSeam({
      overrides: {
        list_accounts: [
          ...mockAccounts,
          makeAccount({ id: "acc-inv", name: "证券账户" }),
          makeAccount({ id: "acc-inv-hidden", name: "隐藏证券户", is_hidden: true }),
        ],
      },
    });
    const store = useReferenceStore();
    await store.refresh();
    expect(store.investmentAccounts.map((a) => a.id)).toEqual(["acc-inv", "acc-inv-hidden"]);
    expect(store.investmentAccountOptions).toEqual([
      { label: "证券账户", value: "acc-inv" },
      { label: "隐藏证券户", value: "acc-inv-hidden" },
    ]);
  });

  it("账户归档（spec #1952 / ADR-0142 决策 4）：读面零过滤——归档账户照进字典、accountMap 可查名，标志随行披露", async () => {
    wireInvokeSeam({
      overrides: {
        list_accounts: [
          ...mockAccounts,
          makeAccount({
            id: "acc-archived",
            name: "借出·张三",
            type: "receivable",
            is_archived: true,
          }),
        ],
      },
    });
    const store = useReferenceStore();
    await store.refresh();
    expect(store.accounts.map((a) => a.id)).toEqual(["acc-1", "acc-2", "acc-archived"]);
    expect(store.accountMap.get("acc-archived")?.name).toBe("借出·张三");
    expect(store.accountMap.get("acc-archived")?.is_archived).toBe(true);
    expect(store.accountMap.get("acc-1")?.is_archived).toBe(false);
  });

  it("可选择账户（spec #1952 / ADR-0142 决策 4）：单一派生 = 账户去归档——选项面排除归档，accounts/accountMap 照旧零过滤", async () => {
    wireInvokeSeam({
      overrides: {
        list_accounts: [
          ...mockAccounts,
          makeAccount({ id: "acc-inv", name: "证券账户" }),
          makeAccount({
            id: "acc-recv-archived",
            name: "借出·张三",
            type: "receivable",
            is_archived: true,
          }),
          makeAccount({ id: "acc-inv-archived", name: "旧证券户", is_archived: true }),
        ],
      },
    });
    const store = useReferenceStore();
    await store.refresh();
    expect(store.accounts.map((a) => a.id)).toEqual([
      "acc-1",
      "acc-2",
      "acc-inv",
      "acc-recv-archived",
      "acc-inv-archived",
    ]);
    expect(store.accountMap.get("acc-recv-archived")?.name).toBe("借出·张三");
    expect(store.selectableAccounts.map((a) => a.id)).toEqual(["acc-1", "acc-2", "acc-inv"]);
    expect(store.selectableAccountOptions).toEqual([
      { label: "现金", value: "acc-1" },
      { label: "招商银行", value: "acc-2" },
      { label: "证券账户", value: "acc-inv" },
    ]);
    expect(store.investmentAccounts.map((a) => a.id)).toEqual(["acc-inv", "acc-inv-archived"]);
    expect(store.selectableInvestmentAccountOptions).toEqual([
      { label: "证券账户", value: "acc-inv" },
    ]);
    expect(store.fundingCandidateAccounts.map((a) => a.id)).toEqual(["acc-1", "acc-2"]);
  });

  it("商户派生映射 merchantMap（含按名字查找 merchantByName）正确", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.merchantMap.get("mch-1")?.name).toBe("京东");
    expect(store.merchantByName.get("红旗连锁")?.id).toBe("mch-2");
  });

  it("保司字典接入（issue #713 / ADR-0082）：在用进字典与按名查找，含已删全量拉取，insurerMap 含软删行", async () => {
    const store = useReferenceStore();
    await store.refresh();
    wireInvokeSeam({
      overrides: {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: mockCategories,
        list_insurers: [
          {
            id: "ins-1",
            name: "平安人寿",
            is_deleted: false,
            updated_at: "",
            version: 1,
            device_id: "test",
          },
          {
            id: "ins-2",
            name: "海峡金桥",
            is_deleted: true,
            updated_at: "",
            version: 1,
            device_id: "test",
          },
        ],
      },
    });
    await store.refresh();

    const insurerCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_insurers");
    expect(insurerCalls.length).toBeGreaterThan(0);
    for (const [, args] of insurerCalls) {
      expect(args).toMatchObject({ includeDeleted: true });
    }
    expect(store.insurers.map((i) => i.id)).toEqual(["ins-1"]);
    expect(store.insurerByName.get("平安人寿")?.id).toBe("ins-1");
    expect(store.insurerByName.get("海峡金桥")).toBeUndefined();
    expect(store.insurerMap.get("ins-1")?.name).toBe("平安人寿");
    expect(store.insurerMap.get("ins-2")?.name).toBe("海峡金桥");
  });

  it("list_merchants 以含软删全量拉取（includeDeleted=true，筛选下拉数据源 issue #191）", async () => {
    const store = useReferenceStore();
    await store.refresh();
    const merchantCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_merchants");
    expect(merchantCalls.length).toBeGreaterThan(0);
    for (const [, args] of merchantCalls) {
      expect(args).toMatchObject({ includeDeleted: true });
    }
  });

  it("软删商户：从字典与选择列表消失（不可再选），merchantMap 仍保留（历史引用照常显示）", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.merchantMap.get("mch-1")?.name).toBe("京东");

    wireInvokeSeam({
      overrides: {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: mockCategories,
        list_merchants: [{ ...mockMerchants[0], is_deleted: true }, mockMerchants[1]],
        list_insurers: mockInsurers,
      },
    });
    await store.refresh();

    expect(store.merchants.map((m) => m.id)).toEqual(["mch-2"]);
    expect(store.merchantByName.get("京东")).toBeUndefined();
    expect(store.merchantMap.get("mch-1")?.name).toBe("京东");
    expect(store.merchantMap.get("mch-2")?.name).toBe("红旗连锁");
  });

  it("分类树派生：rootCategories/expenseCategories/incomeCategories", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.rootCategories.map((c) => c.id)).toEqual(["cat-root", "cat-income"]);
    expect(store.expenseCategories.map((c) => c.id)).toEqual(["cat-root", "cat-child"]);
    expect(store.incomeCategories.map((c) => c.id)).toEqual(["cat-income"]);
  });

  it("categoryChildren/categoryPath/treeCategoryOptions 正确", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.categoryChildren("cat-root").map((c) => c.id)).toEqual(["cat-child"]);
    expect(store.categoryPath("cat-child")).toBe("餐饮 > 外卖");
    expect(store.categoryPath("cat-root")).toBe("餐饮");
    expect(store.categoryPath(null)).toBe("");
    expect(store.categoryPath("cat-gone")).toBe("");
    expect(store.categoryPath("cat-del")).toBe("餐饮 > 旅游");
    const expenseTree = store.treeCategoryOptions("expense");
    expect(expenseTree.map((n) => n.key)).toEqual(["cat-root"]);
    expect(expenseTree[0].children?.map((c) => c.key)).toEqual(["cat-child"]);
    expect(store.treeCategoryOptions("income").map((n) => n.key)).toEqual(["cat-income"]);
  });

  it("分类路径缓存随失效重拉同步（重命名 / 软删后路径即时更新，issue #2187）", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.categoryPath("cat-child")).toBe("餐饮 > 外卖");

    wireInvokeSeam({
      overrides: {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: [
          mockCategories[0],
          { ...mockCategories[1], name: "快餐" },
          ...mockCategories.slice(2),
        ],
        list_merchants: mockMerchants,
        list_insurers: mockInsurers,
      },
    });
    await store.refresh();
    expect(store.categoryPath("cat-child")).toBe("餐饮 > 快餐");

    wireInvokeSeam({
      overrides: {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: [
          mockCategories[0],
          { ...mockCategories[1], name: "快餐", is_deleted: true },
          ...mockCategories.slice(2),
        ],
        list_merchants: mockMerchants,
        list_insurers: mockInsurers,
      },
    });
    await store.refresh();
    expect(store.categories.map((c) => c.id)).toEqual(["cat-root", "cat-income"]);
    expect(store.categoryMap.get("cat-child")?.name).toBe("快餐");
    expect(store.categoryMap.get("cat-del")?.name).toBe("旅游");
    expect(store.categoryPath("cat-child")).toBe("餐饮 > 快餐");
  });

  it("categoryDisplayName：子分类路径名、顶级自身名，解析不到回退兜底名（issue #356）", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.categoryDisplayName("cat-child", "未分类")).toBe("餐饮 > 外卖");
    expect(store.categoryDisplayName("cat-root", "未分类")).toBe("餐饮");
    expect(store.categoryDisplayName("cat-gone", "未分类")).toBe("未分类");
    expect(store.categoryDisplayName(null, "未分类")).toBe("未分类");
  });

  it("getCurrency 按 code 返回币种", async () => {
    const store = useReferenceStore();
    await store.refresh();
    expect(store.getCurrency("CNY")).toStrictEqual(mockCurrencies[0]);
    expect(store.getCurrency("EUR")).toBeUndefined();
  });
});
