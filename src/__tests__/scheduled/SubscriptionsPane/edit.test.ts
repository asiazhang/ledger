import { describe, it, expect, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { NModal, NInputNumber, NDatePicker, NSelect, NTreeSelect } from "naive-ui";
import { useReferenceStore } from "@/stores/reference";
import { mockInvoke } from "@ledger/test-support/invoke-mock";
import { formatAmount } from "@ledger/money";
import { refCurrencies } from "@ledger/test-support/reference-stubs";
import { makeSubscriptionPlan } from "../../factories";
import {
  mockAccounts,
  mockDetails,
  makeDetail,
  mockMerchants,
  findInput,
  mountView,
  setFailSubscriptionUpdate,
  setMockMerchants,
  setMockAccounts,
  setMockPlans,
  setup,
} from "./common";
import type { Account } from "@ledger/types";

const cny = refCurrencies[0];

beforeEach(setup);

describe("SubscriptionsPane 订阅编辑——仅非金额字段（issue #162）", () => {
  function findModal(wrapper: ReturnType<typeof mount>, title: string) {
    const modal = wrapper.findAllComponents(NModal).find((m) => m.props("title") === title);
    expect(modal, `应存在标题为「${title}」的弹窗`).toBeDefined();
    return modal!;
  }

  async function openEditModal(wrapper: ReturnType<typeof mount>) {
    await wrapper.find('[data-testid="op-edit-a1"]').trigger("click");
    await flushPromises();
  }

  it("进行中/已暂停行提供编辑入口，已取消行不提供", async () => {
    const plan = makeSubscriptionPlan({ id: "a1" });
    setMockPlans([
      plan,
      makeSubscriptionPlan({ id: "c1", status: "cancelled", note: "已取消订阅" }),
    ]);
    mockDetails.set("a1", makeDetail(plan, []));
    const wrapper = await mountView();
    expect(wrapper.find('[data-testid="op-edit-a1"]').exists()).toBe(true);
    await wrapper.find('[data-testid="filter-cancelled"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="op-edit-c1"]').exists()).toBe(false);
  });

  it("编辑弹窗预填非金额字段且无金额输入", async () => {
    const plan = makeSubscriptionPlan({
      id: "a1",
      note: "视频会员",
      category_id: "cat-1",
      account_id: "acc-1",
      amount_cents: 1500,
    });
    setMockPlans([plan]);
    mockDetails.set("a1", makeDetail(plan, []));
    const wrapper = await mountView();
    await openEditModal(wrapper);
    const modal = findModal(wrapper, "编辑订阅");
    expect(modal.props("show")).toBe(true);
    expect(modal.props("title")).toBe("编辑订阅");
    expect(findInput(wrapper, "sub-edit-note").element.value).toBe("视频会员");
    expect(wrapper.findComponent('[data-testid="sub-amount"]').exists()).toBe(false);
    expect(modal.findComponent(NInputNumber).exists()).toBe(false);
    expect(modal.findComponent(NDatePicker).exists()).toBe(false);
    expect(modal.text()).not.toContain(formatAmount(1500, cny));
  });

  it("未选账户时不提交编辑", async () => {
    const plan = makeSubscriptionPlan({ id: "a1" });
    setMockPlans([plan]);
    mockDetails.set("a1", makeDetail(plan, []));
    const wrapper = await mountView();
    await openEditModal(wrapper);
    wrapper.findComponent(NSelect).vm.$emit("update:value", null);
    await flushPromises();
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger("click");
    await flushPromises();
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "update_scheduled_subscription")).toBe(
      false,
    );
  });

  it("提交编辑走订阅编辑命令，参数不含金额字段，成功后关闭弹窗并刷新清单", async () => {
    const plan = makeSubscriptionPlan({ id: "a1", note: "视频会员" }, "mer-1");
    setMockPlans([plan]);
    mockDetails.set("a1", makeDetail(plan, []));
    const wrapper = await mountView();
    await openEditModal(wrapper);
    const noteInput = findInput(wrapper, "sub-edit-note");
    await noteInput.setValue("音乐会员");
    await noteInput.trigger("input");
    wrapper.findComponent(NSelect).vm.$emit("update:value", "acc-1");
    wrapper.findComponent(NTreeSelect).vm.$emit("update:value", "cat-1");
    await flushPromises();
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger("click");
    await flushPromises();
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "update_scheduled_subscription");
    expect(call).toBeDefined();
    expect(call![1]).toEqual({
      input: {
        id: "a1",
        account_id: "acc-1",
        category_id: "cat-1",
        merchant_id: "mer-1",
        note: "音乐会员",
      },
    });
    expect(findModal(wrapper, "编辑订阅").props("show")).toBe(false);
    expect(wrapper.text()).toContain("音乐会员");
  });

  it("编辑弹窗改商户：预填当前商户，保存携带新 merchant_id（issue #190）", async () => {
    setMockMerchants([
      ...mockMerchants,
      {
        id: "mer-2",
        name: "商户B",
        updated_at: "2026-01-01T00:00:00Z",
        version: 1,
        device_id: "test",
        is_deleted: false,
      },
    ]);
    await useReferenceStore().refresh();
    const plan = makeSubscriptionPlan({ id: "a1", note: "视频会员" }, "mer-1");
    setMockPlans([plan]);
    mockDetails.set("a1", makeDetail(plan, []));
    const wrapper = await mountView();
    await openEditModal(wrapper);
    const merchantSelect = wrapper
      .findComponent('[data-testid="sub-edit-merchant"]')
      .findComponent(NSelect);
    expect(merchantSelect.exists()).toBe(true);
    expect(merchantSelect.props("value")).toBe("mer-1");
    merchantSelect.vm.$emit("update:value", "mer-2");
    await flushPromises();
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger("click");
    await flushPromises();
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "update_scheduled_subscription");
    expect(call![1]).toMatchObject({ input: { merchant_id: "mer-2" } });
    expect(wrapper.text()).toContain("商户B");
  });

  it("挂保单的缴费协议编辑弹窗不显示商户字段（issue #713 / ADR-0082：付款对象语义由保司承担）", async () => {
    const plan = {
      ...makeSubscriptionPlan({ id: "a1", note: "重疾险年缴" }),
      policy_id: "policy-1",
    };
    setMockPlans([plan]);
    mockDetails.set("a1", makeDetail(plan, []));
    const wrapper = await mountView();
    await openEditModal(wrapper);
    expect(wrapper.findComponent('[data-testid="sub-edit-merchant"]').exists()).toBe(false);
  });

  it("原商户软删且不在字典：下拉兜底选项承载原 id，未改动提交仍携带原 id（接缝软删兜底分支接线）", async () => {
    const plan = makeSubscriptionPlan({ id: "a1", note: "视频会员" }, "mer-gone");
    setMockPlans([plan]);
    mockDetails.set("a1", makeDetail(plan, []));
    setMockMerchants([]);
    await useReferenceStore().refresh();
    const wrapper = await mountView();
    await openEditModal(wrapper);
    const merchantSelect = wrapper
      .findComponent('[data-testid="sub-edit-merchant"]')
      .findComponent(NSelect);
    expect(merchantSelect.props("value")).toBe("mer-gone");
    const options = merchantSelect.props("options") as { value: string }[];
    expect(options.some((o) => o.value === "mer-gone")).toBe(true);
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger("click");
    await flushPromises();
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "update_scheduled_subscription");
    expect(call).toBeDefined();
    expect(call![1]).toMatchObject({ input: { merchant_id: "mer-gone" } });
  });

  it("提交失败时弹窗保持打开", async () => {
    const plan = makeSubscriptionPlan({ id: "a1" });
    setMockPlans([plan]);
    mockDetails.set("a1", makeDetail(plan, []));
    const wrapper = await mountView();
    setFailSubscriptionUpdate(true);
    await openEditModal(wrapper);
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger("click");
    await flushPromises();
    expect(findModal(wrapper, "编辑订阅").props("show")).toBe(true);
  });

  it("编辑挂归档账户的订阅：账户下拉补回该账户并标「已归档」、保持原选中（编辑实例选项面）", async () => {
    const archived: Account = {
      id: "acc-arch",
      name: "旧工资卡",
      type: "cash",
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
    const plan = makeSubscriptionPlan({ id: "a1", account_id: "acc-arch" });
    setMockPlans([plan]);
    mockDetails.set("a1", makeDetail(plan, []));
    setMockAccounts([...mockAccounts, archived]);
    await useReferenceStore().refresh();
    const wrapper = await mountView();
    await openEditModal(wrapper);
    const accountSelect = findModal(wrapper, "编辑订阅").findComponent(NSelect);
    expect(accountSelect.props("value")).toBe("acc-arch");
    expect(accountSelect.props("options")).toContainEqual({
      label: "旧工资卡（已归档）",
      value: "acc-arch",
    });
  });
});
