import { describe, it, expect, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { NModal, NSelect, NTreeSelect, NDatePicker } from "naive-ui";
import { findInput, mountView, setup } from "./common";
import { mockInvoke } from "@ledger/test-support/invoke-mock";
import { componentVm } from "@ledger/test-support/component-vm";

beforeEach(setup);

describe("SubscriptionsPane 新建订阅模态对话框（issue #158）", () => {
  async function openCreateModal(wrapper: ReturnType<typeof mount>) {
    await wrapper.find('[data-testid="sub-create-open"]').trigger("click");
    await flushPromises();
  }

  it("初始无弹窗，点击「新建订阅」打开模态对话框", async () => {
    const wrapper = await mountView();
    const modal = wrapper.findComponent(NModal);
    expect(modal.props("show")).toBe(false);
    await openCreateModal(wrapper);
    expect(modal.props("show")).toBe(true);
    expect(modal.props("title")).toBe("新建订阅");
  });

  it("创建成功后重置表单，重新打开为全新表单", async () => {
    const wrapper = await mountView();
    await openCreateModal(wrapper);
    const noteInput = findInput(wrapper, "sub-note");
    await noteInput.setValue("音乐订阅");
    await noteInput.trigger("input");
    wrapper.findComponent(NSelect).vm.$emit("update:value", "acc-1");
    await flushPromises();
    const amountInput = findInput(wrapper, "sub-amount");
    await amountInput.setValue("25");
    await amountInput.trigger("input");
    await wrapper.findComponent('[data-testid="sub-create"]').trigger("click");
    await flushPromises();
    await openCreateModal(wrapper);
    expect(findInput(wrapper, "sub-note").element.value).toBe("");
  });

  it("仅关闭弹窗（不提交）不触发创建", async () => {
    const wrapper = await mountView();
    await openCreateModal(wrapper);
    wrapper.findComponent(NModal).vm.$emit("update:show", false);
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("show")).toBe(false);
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "create_scheduled_transaction")).toBe(
      false,
    );
  });

  it("弹窗内填表创建走创建命令，金额转分、kind=subscription（公共字段与商户解析断言留给接缝直测）", async () => {
    const wrapper = await mountView();
    await openCreateModal(wrapper);
    const noteInput = findInput(wrapper, "sub-note");
    await noteInput.setValue("音乐订阅");
    await noteInput.trigger("input");
    const amountInput = findInput(wrapper, "sub-amount");
    await amountInput.setValue("25");
    await amountInput.trigger("input");
    wrapper.findComponent(NSelect).vm.$emit("update:value", "acc-1");
    wrapper.findComponent(NTreeSelect).vm.$emit("update:value", "cat-1");
    wrapper.findComponent(NDatePicker).vm.$emit("update:formatted-value", "2026-02-15");
    await flushPromises();

    const createBtn = wrapper.findComponent('[data-testid="sub-create"]');
    await createBtn.trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_scheduled_transaction");
    expect(call).toBeDefined();
    expect(call![1]).toMatchObject({
      input: {
        kind: "subscription",
        account_id: "acc-1",
        category_id: "cat-1",
        amount_cents: 2500,
        note: "音乐订阅",
      },
    });
  });

  it("未选账户时不提交创建", async () => {
    const wrapper = await mountView();
    await openCreateModal(wrapper);
    const amountInput = findInput(wrapper, "sub-amount");
    await amountInput.setValue("25");
    await amountInput.trigger("input");
    await wrapper.findComponent('[data-testid="sub-create"]').trigger("click");
    await flushPromises();
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "create_scheduled_transaction")).toBe(
      false,
    );
  });

  it("创建成功后关闭弹窗并刷新清单，新订阅出现在列表", async () => {
    const wrapper = await mountView();
    await openCreateModal(wrapper);
    const noteInput = findInput(wrapper, "sub-note");
    await noteInput.setValue("云存储");
    await noteInput.trigger("input");
    const amountInput = findInput(wrapper, "sub-amount");
    await amountInput.setValue("6");
    await amountInput.trigger("input");
    wrapper.findComponent(NSelect).vm.$emit("update:value", "acc-1");
    await flushPromises();
    await wrapper.findComponent('[data-testid="sub-create"]').trigger("click");
    await flushPromises();
    expect(wrapper.findComponent(NModal).props("show")).toBe(false);
    expect(wrapper.text()).toContain("云存储");
  });

  it("商户下拉补全在用商户：选中后创建携带 merchant_id（issue #190）", async () => {
    const wrapper = await mountView();
    await openCreateModal(wrapper);
    const merchantSelect = wrapper
      .findComponent('[data-testid="sub-merchant"]')
      .findComponent(NSelect);
    expect(merchantSelect.exists()).toBe(true);
    const options = merchantSelect.props("options") as { label: string; value: string }[];
    expect(options.map((o) => o.label)).toEqual(["视频平台"]);
    merchantSelect.vm.$emit("update:value", "mer-1");
    const amountInput = findInput(wrapper, "sub-amount");
    await amountInput.setValue("25");
    await amountInput.trigger("input");
    wrapper.findComponent(NSelect).vm.$emit("update:value", "acc-1");
    await flushPromises();
    await wrapper.findComponent('[data-testid="sub-create"]').trigger("click");
    await flushPromises();
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_scheduled_transaction");
    expect(call![1]).toMatchObject({ input: { merchant_id: "mer-1" } });
    expect(wrapper.text()).toContain("视频平台");
  });

  it("输入不存在的商户名保存即建：解析全仓单点走表单接缝，此处仅验接线（选中/即建矩阵见 useScheduledPlanForm.test.ts）", async () => {
    const wrapper = await mountView();
    await openCreateModal(wrapper);
    componentVm(wrapper.findComponent('[data-testid="sub-merchant"]')).$emit(
      "update:value",
      "新商户",
    );
    const amountInput = findInput(wrapper, "sub-amount");
    await amountInput.setValue("25");
    await amountInput.trigger("input");
    wrapper.findComponent(NSelect).vm.$emit("update:value", "acc-1");
    await flushPromises();
    await wrapper.findComponent('[data-testid="sub-create"]').trigger("click");
    await flushPromises();
    const createCall = mockInvoke.mock.calls.find(
      ([cmd]) => cmd === "create_scheduled_transaction",
    );
    expect(createCall![1]).toMatchObject({ input: { merchant_id: "mer-new-新商户" } });
  });
});
