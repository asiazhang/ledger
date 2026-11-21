import {
  mountView,
  listCalls,
  lastListFilter,
  tablePagination,
  openCreateDropdown,
} from "./common";
import { mockInvoke } from "@ledger/test-support/invoke-mock";
import { describe, it, expect } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { NSelect, NButton, NModal, NInput, NRadioGroup } from "naive-ui";
import CategoryForm from "@/categories/CategoryForm.vue";
import TransferForm from "@/transaction/TransferForm.vue";
import LendingForm from "@/transaction/LendingForm.vue";
import InvestmentForm from "@/investment/InvestmentForm.vue";
import TransactionForm from "@/transaction/TransactionForm.vue";

describe("TransactionsView 记一笔 Modal（issue #141）", () => {
  async function openCreateModal(wrapper: ReturnType<typeof mount>) {
    const btn = wrapper.findAll("button").find((b) => b.text().includes("记一笔"))!;
    await btn.trigger("click");
    await flushPromises();
  }

  it("点击「记一笔」主体直接打开「记一笔 · 支出」弹窗，仅渲染支出子表单（无类型单选）", async () => {
    const wrapper = await mountView();
    expect(wrapper.findComponent(NModal).props("show")).toBe(false);
    expect(wrapper.findComponent(TransactionForm).exists()).toBe(false);
    await openCreateModal(wrapper);
    expect(wrapper.findComponent(NModal).props("show")).toBe(true);
    expect(wrapper.findComponent(NModal).props("title")).toBe("记一笔 · 支出");
    const form = wrapper.findComponent(TransactionForm);
    expect(form.exists()).toBe(true);
    expect(form.props("kind")).toBe("expense");
    expect(wrapper.findComponent(NRadioGroup).exists()).toBe(false);
    expect(form.findComponent(CategoryForm).exists()).toBe(true);
    expect(form.findComponent(TransferForm).exists()).toBe(false);
    expect(form.findComponent(InvestmentForm).exists()).toBe(false);
  });

  it("提交成功后弹窗关闭、回到第 1 页并立即刷新（新记录可见）", async () => {
    const wrapper = await mountView();
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    await openCreateModal(wrapper);
    const before = listCalls().length;
    wrapper.findComponent(TransactionForm).vm.$emit("created");
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("show")).toBe(false);
    expect(listCalls().length).toBe(before + 1);
    expect(lastListFilter()).toMatchObject({ page: 1 });
  });

  it("真实提交链路：弹窗内填表提交 → create_transaction → 弹窗关闭并刷新", async () => {
    const wrapper = await mountView();
    await openCreateModal(wrapper);
    const form = wrapper.findComponent(TransactionForm);
    form.getComponent(NInput).vm.$emit("update:value", "12.5");
    form.findAllComponents(NSelect)[1].vm.$emit("update:value", "acc-1");
    await flushPromises();
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === "create_transaction" ? Promise.resolve("new-id") : base(cmd, args),
    );
    const submitBtn = form.findAllComponents(NButton).find((b) => b.text().includes("记支出"))!;
    await submitBtn.trigger("click");
    await flushPromises();
    const createCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction");
    expect(createCalls).toHaveLength(1);
    const [, args] = createCalls[0] as [string, { input: Record<string, unknown> }];
    expect(args.input).toMatchObject({
      kind: "expense",
      amount_cents: 1250,
      currency_code: "CNY",
      account_id: "acc-1",
    });
    expect(wrapper.findComponent(NModal).props("show")).toBe(false);
    expect(lastListFilter()).toMatchObject({ page: 1 });
  });

  it("仅关闭弹窗（不提交）不触发列表刷新", async () => {
    const wrapper = await mountView();
    await openCreateModal(wrapper);
    const before = listCalls().length;
    wrapper.findComponent(NModal).vm.$emit("update:show", false);
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("show")).toBe(false);
    expect(listCalls().length).toBe(before);
  });
});

describe("TransactionsView 记一笔分裂按钮（issue #150）", () => {
  async function clickDropdownItem(label: string) {
    const item = [...document.body.querySelectorAll(".n-dropdown-option")].find(
      (el) => el.textContent?.trim() === label,
    );
    expect(item, `下拉菜单中应存在「${label}」项`).toBeDefined();
    const body = item!.querySelector(".n-dropdown-option-body") as HTMLElement;
    expect(body).toBeDefined();
    body.click();
    await flushPromises();
  }

  it("下拉菜单为 5 项：3 个 kind 项标注快捷键（支出 a/收入 i/转账 z），分隔线后借贷两项（借出/借入），无退款与转换（issue #150/#153/#374/#1048；ADR-0135 决策 5 收窄后买入/卖出不在交易页入口）", async () => {
    const wrapper = await mountView();
    const labels = await openCreateDropdown(wrapper);
    expect(labels).toEqual(["支出 a", "收入 i", "转账 z", "借出", "借入"]);
    expect(labels).not.toContain("退款");
    expect(labels.some((label) => label.includes("转换"))).toBe(false);
  });

  it.each([
    ["支出 a", "支出", "expense"],
    ["收入 i", "收入", "income"],
    ["转账 z", "转账", "transfer"],
  ] as const)("点菜单项「%s」打开对应类型弹窗（无类型单选组）", async (label, kindLabel, kind) => {
    const wrapper = await mountView();
    await openCreateDropdown(wrapper);
    await clickDropdownItem(label);
    expect(wrapper.findComponent(NModal).props("show")).toBe(true);
    expect(wrapper.findComponent(NModal).props("title")).toBe(`记一笔 · ${kindLabel}`);
    const form = wrapper.findComponent(TransactionForm);
    expect(form.props("kind")).toBe(kind);
    expect(wrapper.findComponent(NRadioGroup).exists()).toBe(false);
  });

  it.each([
    ["借出", "lend"],
    ["借入", "borrow"],
  ] as const)(
    "点借贷菜单项「%s」打开借贷变体弹窗（issue #374：kind=lend/borrow 预置方向，不新增交易类型）",
    async (label, kind) => {
      const wrapper = await mountView();
      await openCreateDropdown(wrapper);
      await clickDropdownItem(label);
      expect(wrapper.findComponent(NModal).props("show")).toBe(true);
      expect(wrapper.findComponent(NModal).props("title")).toBe(`记一笔 · ${label}`);
      const form = wrapper.findComponent(TransactionForm);
      expect(form.props("kind")).toBe(kind);
      expect(form.findComponent(LendingForm).exists()).toBe(true);
      expect(form.findComponent(TransferForm).exists()).toBe(false);
    },
  );

  it("下拉展开后再点主体，仍直接打开支出弹窗（两击区互不干扰）", async () => {
    const wrapper = await mountView();
    await openCreateDropdown(wrapper);
    const btn = wrapper.findAll("button").find((b) => b.text().includes("记一笔"))!;
    await btn.trigger("click");
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("title")).toBe("记一笔 · 支出");
  });
});
