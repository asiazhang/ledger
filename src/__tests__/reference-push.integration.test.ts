import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { captureListenHandlers, type CapturedListener } from "@ledger/test-support/listen-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { NDialogProvider, NSelect, NTreeSelect } from "naive-ui";
import { h, reactive } from "vue";
import { useReferenceStore } from "@/stores/reference";
import { makeTransaction } from "./factories";
import CategoryManager from "@/categories/CategoryManager.vue";
import CategoryForm from "@/categories/CategoryForm.vue";
import TransactionsView from "@/views/TransactionsView.vue";
import type { Account, Category, Transaction } from "@ledger/types";

const routeMock = reactive<{ query: Record<string, string | string[] | null> }>({ query: {} });
vi.mock("vue-router", () => ({
  useRoute: () => routeMock,
  useRouter: () => ({ push: () => {} }),
}));

let changedHandlers: CapturedListener[] = [];

function optionValues(
  select: { props(key: string): unknown },
  pick: "value" | "key" | "label",
): string[] {
  const opts = select.props("options");
  expect(Array.isArray(opts), "options 应为数组").toBe(true);
  return (opts as Array<Record<string, unknown>>).map((o) => o[pick] as string);
}

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
    id: "cat-food",
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
    id: "cat-salary",
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
];

const importedAccount: Account = {
  id: "acc-ai",
  name: "AI 导入账户",
  type: "bank",
  currency_code: "USD",
  initial_balance_cents: 0,
  created_at: "2026-02-01T00:00:00Z",
  updated_at: "2026-02-01T00:00:00Z",
  version: 1,
  device_id: "test",
  is_deleted: false,
  is_hidden: false,
  is_archived: false,
};
const importedCategory: Category = {
  id: "cat-ai",
  name: "AI 导入分类",
  kind: "expense",
  parent_id: null,
  icon: null,
  sort_order: 1,
  created_at: "2026-02-01T00:00:00Z",
  updated_at: "2026-02-01T00:00:00Z",
  version: 1,
  device_id: "test",
  is_deleted: false,
};

function listOverrides(
  overrides: {
    accounts?: Account[];
    categories?: Category[];
  } = {},
) {
  return {
    list_accounts: overrides.accounts ?? mockAccounts,
    list_categories: overrides.categories ?? mockCategories,
    list_policies: [] as unknown[],
  };
}

beforeEach(async () => {
  wireInvokeSeam({ overrides: listOverrides() });
  changedHandlers = captureListenHandlers();
  const store = useReferenceStore();
  await store.refresh();
});

afterEach(() => {
  changedHandlers = [];
});

async function simulateExternalWrite(patch: { accounts?: Account[]; categories?: Category[] }) {
  wireInvokeSeam({ overrides: listOverrides(patch) });
  changedHandlers.forEach((h) => h({ payload: undefined }));
  await flushPromises();
}

describe("组件层反应性：mock ledger:changed 使界面/选项原地更新（issue #86 AC2）", () => {
  it("分类管理树：外部 AI 导入新分类后，已打开的分类树原地出现新分类", async () => {
    const wrapper = mount(CategoryManager);
    expect(wrapper.text()).toContain("餐饮");
    expect(wrapper.text()).not.toContain("AI 导入分类");

    await simulateExternalWrite({ categories: [...mockCategories, importedCategory] });

    expect(wrapper.text()).toContain("AI 导入分类");
    expect(wrapper.text()).toContain("餐饮");
  });

  it("表单选项：外部 AI 导入新账户后，已打开的交易表单账户下拉原地出现新选项", async () => {
    const wrapper = mount(CategoryForm, { props: { kind: "expense", submitLabel: "记支出" } });
    const accountSelect = wrapper.findAllComponents(NSelect)[1];
    expect(optionValues(accountSelect, "value")).toEqual(["acc-1", "acc-2"]);

    await simulateExternalWrite({ accounts: [...mockAccounts, importedAccount] });

    expect(optionValues(accountSelect, "value")).toEqual(["acc-1", "acc-2", "acc-ai"]);
    expect(optionValues(accountSelect, "label")).toContain("AI 导入账户");
  });

  it("表单选项：外部 AI 导入新分类后，分类树选择器原地出现新分类", async () => {
    const wrapper = mount(CategoryForm, { props: { kind: "expense", submitLabel: "记支出" } });
    const treeSelect = wrapper.findAllComponents(NTreeSelect)[0];
    expect(optionValues(treeSelect, "key")).toEqual(["cat-food"]);

    await simulateExternalWrite({ categories: [...mockCategories, importedCategory] });

    expect(optionValues(treeSelect, "key")).toEqual(["cat-food", "cat-ai"]);
  });

  it("交易列表映射渲染：外部 AI 更新分类名后，已打开的交易列表分类列原地显示新名称", async () => {
    const txnDb: Transaction[] = [
      makeTransaction({
        id: "txn-001",
        amount_cents: 100,
        amount_native_cents: 100,
        note: "备注 1",
        category_id: "cat-food",
      }),
      makeTransaction({
        id: "txn-002",
        amount_cents: 200,
        amount_native_cents: 200,
        note: "备注 2",
        category_id: "cat-food",
      }),
    ];
    wireInvokeSeam({
      overrides: {
        ...listOverrides(),
        list_transactions: () => Promise.resolve({ items: txnDb, total: txnDb.length }),
      },
    });

    const wrapper = mount(NDialogProvider, {
      slots: { default: () => h(TransactionsView) },
    });
    await flushPromises();
    expect(wrapper.text()).toContain("餐饮");

    const renamed = mockCategories.map((c) => (c.id === "cat-food" ? { ...c, name: "夜宵" } : c));
    await simulateExternalWrite({ categories: renamed });

    expect(wrapper.text()).toContain("夜宵");
    expect(wrapper.text()).not.toContain("餐饮");
  });

  it("stale-while-revalidate：重拉期间界面保留旧数据，完成后整体替换", async () => {
    const wrapper = mount(CategoryManager);
    expect(wrapper.text()).toContain("餐饮");

    let resolveCats!: (v: Category[]) => void;
    wireInvokeSeam({
      overrides: {
        ...listOverrides(),
        list_categories: () =>
          new Promise((res) => {
            resolveCats = res;
          }),
      },
    });

    changedHandlers.forEach((h) => h({ payload: undefined }));
    await flushPromises();
    expect(wrapper.text()).toContain("餐饮");
    expect(wrapper.text()).not.toContain("AI 导入分类");

    resolveCats([...mockCategories, importedCategory]);
    await flushPromises();

    expect(wrapper.text()).toContain("AI 导入分类");
    expect(wrapper.text()).toContain("餐饮");
  });
});

describe("端到端整合验证：外部 AI 导入账户/分类 → 已打开界面自动呈现新数据（issue #86 AC1）", () => {
  it("批量导入新账户+新分类：已打开的分类树与账户下拉同步原地更新，失效信号可观测", async () => {
    const treeWrapper = mount(CategoryManager);
    const formWrapper = mount(CategoryForm, { props: { kind: "expense", submitLabel: "记支出" } });
    expect(treeWrapper.text()).toContain("餐饮");
    expect(treeWrapper.text()).not.toContain("AI 导入");
    const accountSelect = formWrapper.findAllComponents(NSelect)[1];
    expect(optionValues(accountSelect, "value")).toEqual(["acc-1", "acc-2"]);
    const store = useReferenceStore();
    expect(store.version).toBe(1);

    await simulateExternalWrite({
      accounts: [...mockAccounts, importedAccount],
      categories: [...mockCategories, importedCategory],
    });

    expect(treeWrapper.text()).toContain("AI 导入分类");
    expect(optionValues(accountSelect, "value")).toContain("acc-ai");
    expect(optionValues(accountSelect, "label")).toContain("AI 导入账户");
    expect(store.version).toBe(2);
    expect(store.status).toBe("ready");
    expect(store.accountMap.get("acc-ai")?.name).toBe("AI 导入账户");
    expect(store.categoryMap.get("cat-ai")?.name).toBe("AI 导入分类");
  });
});
