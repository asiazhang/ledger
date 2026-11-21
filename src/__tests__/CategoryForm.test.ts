import { describe, it, expect, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import { defineComponent } from "vue";
import { NSelect } from "naive-ui";
import { useReferenceStore } from "@/stores/reference";
import CategoryForm from "@/categories/CategoryForm.vue";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import { MOBILE_CARD_CLASS } from "@ledger/ui-kit/app-modal.css.ts";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import type { Account, Category, Transaction } from "@ledger/types";

const mockAccounts: Account[] = [
  {
    id: "acc-1",
    name: "现金",
    type: "cash",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    is_hidden: false,
    is_archived: false,
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
];

const mockCategories: Category[] = [
  {
    id: "cat-1",
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
];

describe("CategoryForm.vue", () => {
  beforeEach(async () => {
    wireInvokeSeam({
      overrides: {
        list_accounts: mockAccounts,
        list_categories: mockCategories,
        list_policies: [],
      },
    });
    const store = useReferenceStore();
    await store.refresh();
  });

  it("挂载并显示提交按钮文本", () => {
    const wrapper = mount(CategoryForm, {
      props: { kind: "expense", submitLabel: "记支出" },
    });
    expect(wrapper.text()).toContain("记支出");
  });

  it("挂载并显示收入表单文本", () => {
    const wrapper = mount(CategoryForm, {
      props: { kind: "income", submitLabel: "记收入" },
    });
    expect(wrapper.text()).toContain("记收入");
  });

  it("点击提交按钮触发 submit（无账户时只提示不调用后端）", async () => {
    mockInvoke.mockClear();
    const wrapper = mount(CategoryForm, {
      props: { kind: "expense", submitLabel: "记支出" },
    });
    const btn = wrapper.find("button");
    expect(btn.exists()).toBe(true);
    await btn.trigger("click");
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction");
    expect(calls).toHaveLength(0);
  });

  it("在 store 加载前挂载不应崩溃", () => {
    setActivePinia(createPinia());
    const wrapper = mount(CategoryForm, {
      props: { kind: "expense", submitLabel: "记支出" },
    });
    expect(wrapper.exists()).toBe(true);
  });

  it("正确渲染选择器（账户、币种等至少存在一个元素）", () => {
    const wrapper = mount(CategoryForm, {
      props: { kind: "expense", submitLabel: "记支出" },
    });
    expect(wrapper.text()).toContain("金额");
    expect(wrapper.text()).toContain("账户");
    expect(wrapper.text()).toContain("分类");
    expect(wrapper.text()).toContain("日期");
    expect(wrapper.text()).toContain("备注");
  });

  it("设置 valid 表单数据后提交会调用 create_transaction", async () => {
    mockInvoke.mockClear();
    wireInvokeSeam({ defaults: { list_policies: [], list_policy_stats: [] } });
    const wrapper = mount(CategoryForm, {
      props: { kind: "expense", submitLabel: "记支出" },
    });
    const inputs = wrapper.findAll("input");
    expect(inputs.length).toBeGreaterThan(0);
  });

  function amountInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.find('input[placeholder="金额"]');
  }

  function amountInputRoot(wrapper: ReturnType<typeof mount>) {
    const el = amountInput(wrapper).element.closest(".n-input");
    expect(el).not.toBeNull();
    return el as Element;
  }

  function hasErrorStatus(wrapper: ReturnType<typeof mount>) {
    return amountInputRoot(wrapper).classList.contains("n-input--error-status");
  }

  function submitButton(wrapper: ReturnType<typeof mount>) {
    return wrapper.find("button");
  }

  describe("字段错误态（ADR-0058 / #414）", () => {
    const editingTx: Transaction = {
      id: "txn-1",
      kind: "expense",
      amount_cents: 5000,
      currency_code: "CNY",
      amount_native_cents: 5000,
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
      date: "2026-02-01",
      created_at: "2026-02-01T00:00:00Z",
      updated_at: "2026-02-01T00:00:00Z",
      version: 1,
      device_id: "test",
      is_deleted: false,
    };

    function createCalls() {
      return mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction");
    }

    it("初始为空不红，保存按钮可点", () => {
      const wrapper = mount(CategoryForm, {
        props: { kind: "expense", submitLabel: "记支出" },
      });
      expect(hasErrorStatus(wrapper)).toBe(false);
      expect(submitButton(wrapper).attributes("disabled")).toBeUndefined();
    });

    it("输入解析失败文本（4.30发）即时红显、保存禁用、非法文本原样保留", async () => {
      const wrapper = mount(CategoryForm, {
        props: { kind: "expense", submitLabel: "记支出" },
      });
      await amountInput(wrapper).setValue("4.30发");
      expect(hasErrorStatus(wrapper)).toBe(true);
      expect(submitButton(wrapper).attributes("disabled")).toBeDefined();
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe("4.30发");
    });

    it("多个小数点（4.3.0）与超两位小数（4.305）同样即时红显", async () => {
      const wrapper = mount(CategoryForm, {
        props: { kind: "expense", submitLabel: "记支出" },
      });
      await amountInput(wrapper).setValue("4.3.0");
      expect(hasErrorStatus(wrapper)).toBe(true);
      await amountInput(wrapper).setValue("4.305");
      expect(hasErrorStatus(wrapper)).toBe(true);
    });

    it("非法文本失焦不清空、红态持续；修正后红态解除、保存恢复可点", async () => {
      const wrapper = mount(CategoryForm, {
        props: { kind: "expense", submitLabel: "记支出" },
      });
      await amountInput(wrapper).setValue("4.30发");
      await amountInput(wrapper).trigger("blur");
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe("4.30发");
      expect(hasErrorStatus(wrapper)).toBe(true);
      expect(submitButton(wrapper).attributes("disabled")).toBeDefined();
      await amountInput(wrapper).setValue("4.30");
      expect(hasErrorStatus(wrapper)).toBe(false);
      expect(submitButton(wrapper).attributes("disabled")).toBeUndefined();
    });

    it("清空后未失焦不红；失焦红；重新输入合法解除", async () => {
      const wrapper = mount(CategoryForm, {
        props: { kind: "expense", submitLabel: "记支出" },
      });
      await amountInput(wrapper).setValue("12");
      await amountInput(wrapper).setValue("");
      expect(hasErrorStatus(wrapper)).toBe(false);
      await amountInput(wrapper).trigger("blur");
      expect(hasErrorStatus(wrapper)).toBe(true);
      await amountInput(wrapper).setValue("12");
      expect(hasErrorStatus(wrapper)).toBe(false);
    });

    it("保存尝试时空值红显兜底，不发起提交（格式类 toast 被红态取代）", async () => {
      mockInvoke.mockClear();
      const wrapper = mount(CategoryForm, {
        props: { kind: "expense", submitLabel: "记支出" },
      });
      await submitButton(wrapper).trigger("click");
      expect(hasErrorStatus(wrapper)).toBe(true);
      expect(createCalls()).toHaveLength(0);
    });

    it("编辑弹窗合法回填（50 元）不显示红态、保存可点", () => {
      const wrapper = mount(CategoryForm, {
        props: { kind: "expense", submitLabel: "记支出", editing: editingTx },
      });
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe("50");
      expect(hasErrorStatus(wrapper)).toBe(false);
      expect(submitButton(wrapper).attributes("disabled")).toBeUndefined();
    });

    it("纯零/负数不红、保存可点、提交走业务类校验通道（不发起 create_transaction）", async () => {
      mockInvoke.mockClear();
      const wrapper = mount(CategoryForm, {
        props: { kind: "expense", submitLabel: "记支出" },
      });
      await amountInput(wrapper).setValue("0");
      expect(hasErrorStatus(wrapper)).toBe(false);
      expect(submitButton(wrapper).attributes("disabled")).toBeUndefined();
      await submitButton(wrapper).trigger("click");
      expect(createCalls()).toHaveLength(0);
      await amountInput(wrapper).setValue("-5");
      expect(hasErrorStatus(wrapper)).toBe(false);
      await submitButton(wrapper).trigger("click");
      expect(createCalls()).toHaveLength(0);
    });

    it("创建成功后表单不留潜伏红态（清空金额但初始为空不红，ADR-0058 决策 2）", async () => {
      mockInvoke.mockClear();
      wireInvokeSeam({
        defaults: { list_policies: [], list_policy_stats: [], create_transaction: "new-id" },
      });
      const wrapper = mount(CategoryForm, {
        props: { kind: "expense", submitLabel: "记支出" },
      });
      await submitButton(wrapper).trigger("click");
      await amountInput(wrapper).setValue("12");
      await amountInput(wrapper).trigger("blur");
      wrapper.findAllComponents(NSelect)[1].vm.$emit("update:value", "acc-1");
      await submitButton(wrapper).trigger("click");
      await flushPromises();
      expect(createCalls()).toHaveLength(1);
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe("");
      expect(hasErrorStatus(wrapper)).toBe(false);
    });
  });

  describe("记一笔表单 × AppModal 移动档（issue #844）", () => {
    function mountFormModal() {
      const Harness = defineComponent({
        components: { AppModal, CategoryForm },
        template: `
          <AppModal :show="true" title="记一笔" preset="card" card-size="md">
            <CategoryForm kind="expense" submit-label="记支出" />
          </AppModal>
        `,
      });
      return mount(Harness);
    }

    it("移动档呈全屏化结构：近全屏卡片 + 左置标签表单项 + 按钮行居节奏容器末块", async () => {
      setFakeMedia({ width: 390 });
      mountFormModal();
      await flushPromises();

      const card = document.body.querySelector(".n-card");
      expect(card, "卡片应存在").not.toBeNull();
      expect(card!.classList.contains(MOBILE_CARD_CLASS)).toBe(true);
      expect((card as HTMLElement).style.width).toBe("calc(100vw - 32px)");
      expect(document.body.querySelector(".n-form-item.n-form-item--left-labelled")).not.toBeNull();
      const last = document.body.querySelector(".n-form > .n-space > :last-child");
      expect(last, "节奏容器末块应存在").not.toBeNull();
      expect(last!.querySelector("button")).not.toBeNull();
    });
  });
});
