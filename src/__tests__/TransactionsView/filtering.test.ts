import {
  routeMock,
  makeTxn,
  setTxnDb,
  setAccountDb,
  setMerchantDb,
  mountView,
  mountMobile,
  cards,
  listCalls,
  lastListFilter,
  bodyRows,
} from "./common";
import { describe, it, expect, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { NSelect, NButton } from "naive-ui";
import { fireProp } from "@ledger/test-support/component-vm";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import { MAIN_LIST_KINDS } from "@/transaction/useTransactionFilter";
import { useReferenceStore } from "@/stores/reference";
import type { Account, Merchant, Transaction } from "@ledger/types";

describe("TransactionsView URL 下钻接线（issue #97/#191，冒烟级）", () => {
  beforeEach(() => {
    setTxnDb([
      makeTxn(1, "acc-1", { merchant_id: "mch-1", date: "2026-01-05" }),
      makeTxn(2, "acc-2", { merchant_id: "mch-1", date: "2026-02-10" }),
      makeTxn(3, "acc-1", { date: "2026-01-20" }),
      makeTxn(4, "acc-2", { kind: "transfer", to_account_id: "acc-1", date: "2026-01-25" }),
    ]);
  });

  it("带有效 account 参数进入时自动按该账户过滤（涉及语义含转入侧）", async () => {
    routeMock.query = { account: "acc-1" };
    const wrapper = await mountView();
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: "acc-1",
    });
    expect(wrapper.text()).toContain("共 3 条");
  });

  it("account 与 merchant 参数可组合直达（同时生效）", async () => {
    routeMock.query = { account: "acc-1", merchant: "mch-1" };
    const wrapper = await mountView();
    expect(lastListFilter()).toMatchObject({
      involving_account_id: "acc-1",
      merchant_id: "mch-1",
    });
    expect(wrapper.text()).toContain("共 1 条");
  });

  it("已挂载时导航清除 account 参数复位为全量并回到第 1 页", async () => {
    routeMock.query = { account: "acc-1" };
    const wrapper = await mountView();
    expect(lastListFilter()).toMatchObject({ involving_account_id: "acc-1", page: 1 });
    routeMock.query = {};
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 1 });
    expect(lastListFilter()).not.toHaveProperty("involving_account_id");
    expect(wrapper.text()).toContain("共 4 条");
  });

  it("kinds 维度（issue #581）下钻直达：请求携带 kinds 数组，且不新增手动控件", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", { kind: "expense", date: "2026-01-05" }),
      makeTxn(2, "acc-2", { kind: "expense", date: "2026-02-10" }),
      makeTxn(3, "acc-1", { kind: "transfer", to_account_id: "acc-2", date: "2026-03-15" }),
    ]);
    const wrapper = await mountView();
    const selectCount = wrapper.findAllComponents(NSelect).length;
    routeMock.query = { category: "none", kinds: "expense,refund" };
    await flushPromises();
    expect(lastListFilter()).toMatchObject({
      uncategorized_only: true,
      kinds: ["expense", "refund"],
    });
    expect(wrapper.text()).toContain("共 2 条");
    expect(wrapper.findAllComponents(NSelect).length).toBe(selectCount);
    routeMock.query = {};
    await flushPromises();
    expect(lastListFilter().kinds).toEqual([...MAIN_LIST_KINDS]);
    expect(wrapper.text()).toContain("共 3 条");
  });
});

describe("TransactionsView 过滤行与手动过滤接线（issue #98，冒烟级）", () => {
  const richDb: Transaction[] = [
    makeTxn(1, "acc-1", { kind: "expense", date: "2026-01-05" }),
    makeTxn(2, "acc-2", { kind: "income", date: "2026-02-10" }),
    makeTxn(3, "acc-1", { kind: "transfer", date: "2026-03-15", to_account_id: "acc-2" }),
    makeTxn(4, "acc-2", { kind: "expense", date: "2026-01-20" }),
    makeTxn(5, "acc-1", { kind: "refund", date: "2026-02-25" }),
  ];

  beforeEach(() => {
    setTxnDb([...richDb]);
  });

  const accountSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[0];
  const merchantSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[1];
  const kindSelect = (wrapper: ReturnType<typeof mount>) => wrapper.findAllComponents(NSelect)[2];

  async function setAccount(wrapper: ReturnType<typeof mount>, id: string | null) {
    accountSelect(wrapper).vm.$emit("update:value", id);
    await flushPromises();
  }

  const clearButton = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NButton).find((b) => b.text().includes("清除筛选"))!;

  it("顶部渲染过滤行：账户/商户/类型下拉可清除、清除筛选按钮（日期起止控件已移除，issue #382）", async () => {
    const wrapper = await mountView();
    const account = accountSelect(wrapper);
    expect(account.props("clearable")).toBe(true);
    expect(
      (account.props("options") as { value: string; label: string }[]).map((o) => o.value),
    ).toEqual(["acc-1", "acc-2"]);
    const merchant = merchantSelect(wrapper);
    expect(merchant.props("clearable")).toBe(true);
    expect((merchant.props("options") as { value: string }[]).map((o) => o.value)).toEqual([
      "mch-1",
    ]);
    const kind = kindSelect(wrapper);
    expect(kind.props("clearable")).toBe(true);
    expect(kind.props("multiple")).toBe(true);
    expect((kind.props("options") as { value: string }[]).map((o) => o.value)).toEqual([
      "income",
      "expense",
      "transfer",
      "refund",
    ]);
    expect(clearButton(wrapper).attributes("disabled")).toBeDefined();
  });

  async function setKind(wrapper: ReturnType<typeof mount>, k: string[] | null) {
    kindSelect(wrapper).vm.$emit("update:value", k);
    await flushPromises();
  }

  it("选择账户即重新查询：意图经模块出口生效，involving_account_id 正确传后端（含转账转入侧）", async () => {
    const wrapper = await mountView();
    const before = listCalls().length;
    await setAccount(wrapper, "acc-2");
    expect(listCalls().length).toBe(before + 1);
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: "acc-2",
    });
    expect(wrapper.text()).toContain("共 3 条");
  });

  it("清除筛选按钮走 resetFilters：复位全部条件并回到全量列表（第 1 页）", async () => {
    const wrapper = await mountView();
    await setAccount(wrapper, "acc-1");
    await setKind(wrapper, ["transfer"]);
    expect(wrapper.text()).toContain("共 1 条");
    await clearButton(wrapper).trigger("click");
    await flushPromises();
    const f = lastListFilter();
    expect(f).toMatchObject({ page: 1, page_size: 20 });
    expect(f).not.toHaveProperty("from");
    expect(f).not.toHaveProperty("to");
    expect(f).not.toHaveProperty("involving_account_id");
    expect(f).not.toHaveProperty("kind");
    expect(wrapper.text()).toContain("共 5 条");
  });

  it("手动改动过滤不回写 URL（组件状态为唯一事实源，与维度无关）", async () => {
    routeMock.query = { account: "acc-1" };
    const wrapper = await mountView();
    await setAccount(wrapper, "acc-2");
    await setKind(wrapper, ["income"]);
    await merchantSelect(wrapper).vm.$emit("update:value", "mch-1");
    await flushPromises();
    expect(routeMock.query).toEqual({ account: "acc-1" });
  });

  it("过滤无结果时展示空态提示（与加载态区分），空态可一键清除", async () => {
    const wrapper = await mountView();
    await setAccount(wrapper, "acc-1");
    await setKind(wrapper, ["income"]);
    expect(wrapper.text()).toContain("没有符合条件的交易");
    expect(bodyRows(wrapper).length).toBe(0);
    await clearButton(wrapper).trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("共 5 条");
  });

  it("类型多选交互路由（spec #1025）：多类型集合生效于请求；满选四通用 kind 归一为默认态（ADR-0135）；清空回默认", async () => {
    const wrapper = await mountView();
    await setKind(wrapper, ["refund", "income"]);
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20, kinds: ["refund", "income"] });
    expect(wrapper.text()).toContain("共 2 条");
    await setKind(wrapper, ["expense", "income"]);
    expect(kindSelect(wrapper).props("value")).toEqual(["income", "expense"]);
    await setKind(wrapper, [...MAIN_LIST_KINDS]);
    expect(lastListFilter().kinds).toEqual([...MAIN_LIST_KINDS]);
    expect(clearButton(wrapper).attributes("disabled")).toBeDefined();
    await setKind(wrapper, null);
    expect(lastListFilter().kinds).toEqual([...MAIN_LIST_KINDS]);
    expect(wrapper.text()).toContain("共 5 条");
  });
});

describe("TransactionsView 商户筛选（issue #191，冒烟级）", () => {
  const merchantDbAll: Merchant[] = [
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
      is_deleted: true,
    },
  ];
  const merchantTxnDb: Transaction[] = [
    makeTxn(1, "acc-1", { merchant_id: "mch-1", date: "2026-01-05" }),
    makeTxn(2, "acc-2", { merchant_id: "mch-1", date: "2026-02-10" }),
    makeTxn(3, "acc-1", { merchant_id: "mch-2", date: "2026-03-15" }),
    makeTxn(4, "acc-1", { merchant_id: null, date: "2026-01-20" }),
  ];

  beforeEach(async () => {
    setMerchantDb(merchantDbAll);
    setTxnDb([...merchantTxnDb]);
    await useReferenceStore().refresh();
  });

  const merchantSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[1];

  async function setMerchant(wrapper: ReturnType<typeof mount>, id: string | null) {
    merchantSelect(wrapper).vm.$emit("update:value", id);
    await flushPromises();
  }

  it("下拉选项含软删商户（仍可过滤历史交易），按名称排序", async () => {
    const wrapper = await mountView();
    const options = merchantSelect(wrapper).props("options") as {
      value: string;
      label: string;
    }[];
    expect(options.map((o) => o.value)).toEqual(["mch-2", "mch-1"]);
    expect(options.map((o) => o.label)).toEqual(["红旗连锁", "京东"]);
  });

  it("选择商户即重新查询：merchant_id 正确传后端，total 随筛选变化", async () => {
    const wrapper = await mountView();
    const before = listCalls().length;
    await setMerchant(wrapper, "mch-1");
    expect(listCalls().length).toBe(before + 1);
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20, merchant_id: "mch-1" });
    expect(wrapper.text()).toContain("共 2 条");
    await setMerchant(wrapper, "mch-2");
    expect(lastListFilter()).toMatchObject({ merchant_id: "mch-2" });
    expect(wrapper.text()).toContain("共 1 条");
    await setMerchant(wrapper, null);
    expect(lastListFilter()).not.toHaveProperty("merchant_id");
    expect(wrapper.text()).toContain("共 4 条");
  });
});

describe("TransactionsView 移动档筛选（issue #846 两档一致）", () => {
  beforeEach(() => {
    setTxnDb([
      makeTxn(1, "acc-1", { merchant_id: "mch-1", date: "2026-01-05" }),
      makeTxn(2, "acc-2", { merchant_id: "mch-1", date: "2026-02-10" }),
      makeTxn(3, "acc-1", { date: "2026-01-20" }),
      makeTxn(4, "acc-2", { kind: "transfer", to_account_id: "acc-1", date: "2026-01-25" }),
    ]);
  });

  it("移动档手动筛选立即生效（setFilter 出口：涉及账户语义含转入侧）", async () => {
    const wrapper = await mountMobile();
    const accountSelect = wrapper.findAllComponents(PinyinSelect)[0].findComponent(NSelect);
    fireProp(accountSelect, "onUpdate:value", "acc-1");
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 1, involving_account_id: "acc-1" });
    expect(cards(wrapper).length).toBe(3);
  });

  it("移动档 URL 下钻：?account= 直达过滤（URL 只读入口同一接线）", async () => {
    routeMock.query = { account: "acc-2" };
    await mountMobile();
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: "acc-2",
    });
  });

  it("移动档空态：过滤无结果提示 + 清除筛选回默认", async () => {
    routeMock.query = { kinds: "income" };
    const wrapper = await mountMobile();
    expect(cards(wrapper).length).toBe(0);
    expect(wrapper.text()).toContain("没有符合条件的交易");
    await wrapper
      .findAll("button")
      .find((b) => b.text() === "清除筛选")!
      .trigger("click");
    await flushPromises();
    expect(cards(wrapper).length).toBe(4);
  });
});

describe("账户归档（spec #1952 / ADR-0142 决策 4）：筛选下拉排除归档，已存筛选值补值", () => {
  const archivedAccount: Account = {
    id: "acc-arch",
    name: "已归档户",
    type: "bank",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    is_hidden: false,
    is_archived: true,
  };
  const activeAccounts: Account[] = [
    { ...archivedAccount, id: "acc-1", name: "现金", type: "cash", is_archived: false },
    { ...archivedAccount, id: "acc-2", name: "银行", is_archived: false },
  ];

  const accountSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[0];

  beforeEach(() => {
    setAccountDb([...activeAccounts, archivedAccount]);
    setTxnDb([
      makeTxn(1, "acc-arch", { date: "2026-01-05" }),
      makeTxn(2, "acc-1", { date: "2026-01-06" }),
    ]);
  });

  it("新建筛选的账户下拉不含归档账户", async () => {
    const wrapper = await mountView();
    expect(
      (accountSelect(wrapper).props("options") as { value: string }[]).map((o) => o.value),
    ).toEqual(["acc-1", "acc-2"]);
  });

  it("当前筛选值恰是归档账户：下拉补回该选项并标「已归档」，值保持选中、过滤照旧生效", async () => {
    routeMock.query = { account: "acc-arch" };
    const wrapper = await mountView();
    const account = accountSelect(wrapper);
    expect(account.props("value")).toBe("acc-arch");
    expect(account.props("options")).toContainEqual({
      label: "已归档户（已归档）",
      value: "acc-arch",
    });
    expect(lastListFilter()).toMatchObject({ page: 1, involving_account_id: "acc-arch" });
    expect(bodyRows(wrapper).length).toBe(1);
  });
});
