import { describe, it, expect, beforeEach } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { NButton, NInput, NInputNumber } from "naive-ui";
import DividendDetail from "@/investment/DividendDetail.vue";
import { makeTransaction } from "./factories";

beforeEach(async () => {
  await wireInvokeSeam({ refreshReferenceStores: true }).ready;
});

describe("DividendDetail 现金分红只读详情（ADR-0109 / #1078）", () => {
  it("归属标的 + 金额 + 到账账户 + 日期 + 备注；无可编辑输入面、无按钮", async () => {
    const wrapper = mount(DividendDetail, {
      props: {
        transaction: makeTransaction({
          id: "txn-dv",
          kind: "dividend",
          amount_cents: 3000,
          amount_native_cents: 3000,
          date: "2026-02-10",
          account_id: "acc-1",
          note: "年度分红",
          source: {
            kind: "instrument",
            entity_id: "inst-dv",
            display_name: "502010 证券基金",
            status: null,
          },
        }),
      },
    });
    await flushPromises();
    expect(wrapper.text()).toContain("分红");
    expect(wrapper.text()).toContain("502010 证券基金");
    expect(wrapper.text()).toContain("2026-02-10");
    expect(wrapper.text()).toContain("现金");
    expect(wrapper.text()).toContain("年度分红");
    expect(wrapper.text()).toContain("30");
    expect(wrapper.text()).not.toContain("3000");
    expect(wrapper.findAllComponents(NInput)).toHaveLength(0);
    expect(wrapper.findAllComponents(NInputNumber)).toHaveLength(0);
    expect(wrapper.findAllComponents(NButton)).toHaveLength(0);
  });

  it("无来源标的回退占位（读投影未填充时不抛错）", async () => {
    const wrapper = mount(DividendDetail, {
      props: {
        transaction: makeTransaction({ id: "txn-dv2", kind: "dividend" }),
      },
    });
    await flushPromises();
    expect(wrapper.text()).toContain("分红");
    expect(wrapper.text()).toContain("—");
  });
});
