import { describe, it, expect, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { NSelect } from "naive-ui";
import { useReferenceStore } from "@/stores/reference";
import RefundForm from "@/transaction/RefundForm.vue";
import type { Transaction } from "@ledger/types";

const fixedTx: Transaction = {
  id: "txn-001",
  kind: "expense",
  amount_cents: 3000,
  currency_code: "CNY",
  amount_native_cents: 3000,
  fx_rate_used: null,
  fx_rate_source: null,
  source_order_no: null,
  account_id: "acc-1",
  to_account_id: null,
  funding_account_id: null,
  fundings: [],
  purchases: [],
  category_id: "cat-1",
  merchant_id: null,
  policy_id: null,
  source: null,
  convert: null,
  refund_of_transaction_id: null,
  note: null,
  date: "2026-01-01",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  version: 1,
  device_id: "test",
  is_deleted: false,
};

describe("RefundForm.vue", () => {
  beforeEach(async () => {
    wireInvokeSeam({
      overrides: {},
    });
    const store = useReferenceStore();
    await store.refresh();
  });

  it("挂载并显示提交按钮文本", () => {
    const wrapper = mount(RefundForm);
    expect(wrapper.text()).toContain("记退款");
  });

  function amountInput(wrapper: ReturnType<typeof mount>) {
    return wrapper.find('input[placeholder="退款金额"]');
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

  function createCalls() {
    return mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction");
  }

  describe("字段错误态（ADR-0058 / #415）", () => {
    it("初始为空不红，保存按钮可点（搜索选择模式）", () => {
      const wrapper = mount(RefundForm);
      expect(hasErrorStatus(wrapper)).toBe(false);
      expect(submitButton(wrapper).attributes("disabled")).toBeUndefined();
    });

    it("输入解析失败文本（4.30发）即时红显、保存禁用、非法文本原样保留", async () => {
      const wrapper = mount(RefundForm);
      await amountInput(wrapper).setValue("4.30发");
      expect(hasErrorStatus(wrapper)).toBe(true);
      expect(submitButton(wrapper).attributes("disabled")).toBeDefined();
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe("4.30发");
    });

    it("多个小数点（4.3.0）与超两位小数（4.305）同样即时红显", async () => {
      const wrapper = mount(RefundForm);
      await amountInput(wrapper).setValue("4.3.0");
      expect(hasErrorStatus(wrapper)).toBe(true);
      await amountInput(wrapper).setValue("4.305");
      expect(hasErrorStatus(wrapper)).toBe(true);
    });

    it("非法文本失焦不清空、红态持续；修正后红态解除、保存恢复可点", async () => {
      const wrapper = mount(RefundForm);
      await amountInput(wrapper).setValue("4.30发");
      await amountInput(wrapper).trigger("blur");
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe("4.30发");
      expect(hasErrorStatus(wrapper)).toBe(true);
      expect(submitButton(wrapper).attributes("disabled")).toBeDefined();
      await amountInput(wrapper).setValue("4.30");
      expect(hasErrorStatus(wrapper)).toBe(false);
      expect(submitButton(wrapper).attributes("disabled")).toBeUndefined();
    });

    it("清空后未失焦不红；失焦红；重新输入合法解除（行内模式回填金额）", async () => {
      const wrapper = mount(RefundForm, { props: { fixedTarget: fixedTx } });
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe("30");
      expect(hasErrorStatus(wrapper)).toBe(false);
      await amountInput(wrapper).setValue("");
      expect(hasErrorStatus(wrapper)).toBe(false);
      await amountInput(wrapper).trigger("blur");
      expect(hasErrorStatus(wrapper)).toBe(true);
      await amountInput(wrapper).setValue("12");
      expect(hasErrorStatus(wrapper)).toBe(false);
    });

    it("保存尝试时空值红显兜底，不发起提交（格式类 toast 被红态取代）", async () => {
      const wrapper = mount(RefundForm);
      await submitButton(wrapper).trigger("click");
      expect(hasErrorStatus(wrapper)).toBe(true);
      expect(createCalls()).toHaveLength(0);
    });

    it("行内模式合法回填（30 元）不显示红态、保存可点", () => {
      const wrapper = mount(RefundForm, { props: { fixedTarget: fixedTx } });
      expect(hasErrorStatus(wrapper)).toBe(false);
      expect(submitButton(wrapper).attributes("disabled")).toBeUndefined();
    });

    it("纯零/负数不红、保存可点、提交走业务类校验通道（不发起 create_transaction）", async () => {
      const wrapper = mount(RefundForm, { props: { fixedTarget: fixedTx } });
      await amountInput(wrapper).setValue("0");
      expect(hasErrorStatus(wrapper)).toBe(false);
      expect(submitButton(wrapper).attributes("disabled")).toBeUndefined();
      await submitButton(wrapper).trigger("click");
      await flushPromises();
      expect(createCalls()).toHaveLength(0);
      await amountInput(wrapper).setValue("-5");
      expect(hasErrorStatus(wrapper)).toBe(false);
      await submitButton(wrapper).trigger("click");
      await flushPromises();
      expect(createCalls()).toHaveLength(0);
    });

    it("创建成功后表单不留潜伏红态（清空金额但初始为空不红，ADR-0058 决策 2）", async () => {
      wireInvokeSeam({ overrides: { create_transaction: "refund-id" } });
      const wrapper = mount(RefundForm, { props: { fixedTarget: fixedTx } });
      await amountInput(wrapper).setValue("");
      await submitButton(wrapper).trigger("click");
      expect(hasErrorStatus(wrapper)).toBe(true);
      expect(createCalls()).toHaveLength(0);
      await amountInput(wrapper).setValue("12");
      await amountInput(wrapper).trigger("blur");
      await submitButton(wrapper).trigger("click");
      await flushPromises();
      expect(createCalls()).toHaveLength(1);
      expect((amountInput(wrapper).element as HTMLInputElement).value).toBe("");
      expect(hasErrorStatus(wrapper)).toBe(false);
      expect(wrapper.emitted("created")).toHaveLength(1);
    });
  });
});

describe("RefundForm.vue 币种随继承目标推导（issue #1770 / ADR-0134 决策 5）", () => {
  const usdFixedTx: Transaction = {
    ...fixedTx,
    id: "txn-usd",
    currency_code: "USD",
    amount_cents: 5000,
    amount_native_cents: 3600,
  };

  function currencySelect(wrapper: ReturnType<typeof mount>) {
    const locked = wrapper.findAllComponents(NSelect).filter((s) => s.props("disabled"));
    expect(locked.length).toBeGreaterThanOrEqual(1);
    return locked[0];
  }

  it("行内模式：币种随原交易币种显示且锁定（不再恒 CNY）", async () => {
    const wrapper = mount(RefundForm, { props: { fixedTarget: usdFixedTx } });
    expect(currencySelect(wrapper).props("value")).toBe("USD");
    expect(currencySelect(wrapper).props("disabled")).toBe(true);
  });

  it("未定目标（搜索模式打开）：币种回落展示币种偏好且锁定", () => {
    const wrapper = mount(RefundForm);
    expect(currencySelect(wrapper).props("value")).toBe("CNY");
    expect(currencySelect(wrapper).props("disabled")).toBe(true);
  });
});
