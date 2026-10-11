import { describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { NDialogProvider } from "naive-ui";
import { h } from "vue";
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import type { TransactionOrderSummary } from "@ledger/types";
import { visibleModalText } from "@ledger/test-support/dom";
import DividendDetail from "@/investment/DividendDetail.vue";
import { makeFunding, makeTransaction } from "../factories";
import {
  cards,
  closeShownModal,
  makeTxn,
  mountMobile,
  mountView,
  openMenuOnRow,
  rowMenuKeys,
  selectRowMenu,
  setTxnDb,
  shownModal,
  SHELL_DEFAULTS,
  SHELL_OVERRIDES,
} from "./common";

/**
 * 订单可读性读侧（issue #1862 / ADR-0138 决策 9；#1928 撤列表徽章）：来源订单号
 * 行菜单与详情订单区。列表不渲染订单号（出口收敛为详情订单区，ADR-0138 修订注记）；
 * 订单区数据经订单汇总只读命令（get_transaction_order_summary）自取。
 */

/** 同单两行：一行组合支付分解、一行单账户普通行——同一订单号。 */
function orderRows() {
  return [
    makeTxn(1, null, {
      kind: "expense",
      note: "组合支付",
      amount_cents: 28160,
      amount_native_cents: 28160,
      source_order_no: "JD-9001",
      fundings: [
        makeFunding({ account_id: "acc-1", amount_cents: 2853, label: "罐装" }),
        makeFunding({ account_id: "acc-2", amount_cents: 25307, label: "固体饼干" }),
      ],
    }),
    makeTxn(2, "acc-1", {
      kind: "expense",
      note: "单账户行",
      amount_cents: 1000,
      amount_native_cents: 1000,
      source_order_no: "JD-9001",
    }),
  ];
}

/** 订单汇总替身应答（与后端聚合口径一致的静态快照）。 */
function orderSummary(): TransactionOrderSummary {
  return {
    source_order_no: "JD-9001",
    row_count: 2,
    total_amount_cents: 29160,
    currency_code: "CNY",
    accounts: [
      { account_id: "acc-1", amount_cents: 3853 },
      { account_id: "acc-2", amount_cents: 25307 },
    ],
    items: [
      makeTransaction({ id: "txn-001", amount_cents: 28160, amount_native_cents: 28160 }),
      makeTransaction({ id: "txn-002", account_id: "acc-1", amount_cents: 1000 }),
    ],
  };
}

async function mountWithOrderSummary() {
  const seam = wireInvokeSeam({
    defaults: SHELL_DEFAULTS,
    overrides: {
      ...SHELL_OVERRIDES,
      get_transaction_order_summary: () => Promise.resolve(orderSummary()),
    },
    refreshReferenceStores: true,
  });
  await seam.ready;
  const wrapper = await mountView();
  return wrapper;
}

describe("列表不渲染订单徽章（#1928 / ADR-0138 修订注记）", () => {
  it("有订单号的行不再渲染「订单 <单号>」徽章（桌面表格，备注照常）", async () => {
    setTxnDb(orderRows());
    const wrapper = await mountView();
    const rows = wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
    expect(rows.length).toBe(2);
    for (const row of rows) expect(row.text()).not.toContain("订单 JD-9001");
    expect(rows[0].text()).toContain("组合支付");
    expect(rows[1].text()).toContain("单账户行");
  });

  it("无备注有单号的行：备注列落 '-'，整行不出现订单号", async () => {
    setTxnDb([makeTxn(4, "acc-1", { note: null, source_order_no: "JD-9001" })]);
    const wrapper = await mountView();
    const row = wrapper.find(".n-data-table-tbody .n-data-table-tr");
    expect(row.text()).not.toContain("JD-9001");
  });

  it("无订单号的行零变化（手动记账行）", async () => {
    setTxnDb([makeTxn(3, "acc-1", { note: "普通支出" })]);
    const wrapper = await mountView();
    const row = wrapper.find(".n-data-table-tbody .n-data-table-tr");
    expect(row.text()).not.toContain("订单 JD");
    expect(row.text()).toContain("普通支出");
  });

  it("移动卡片同源撤徽章：有单号卡与无单号卡都不出现订单号", async () => {
    setTxnDb([...orderRows(), makeTxn(3, "acc-1")]);
    const wrapper = await mountMobile();
    const list = cards(wrapper);
    expect(list[0].text()).not.toContain("订单 JD-9001");
    expect(list[1].text()).not.toContain("订单 JD-9001");
    expect(list[2].text()).not.toContain("订单 JD");
  });
});

describe("订单行菜单（issue #1862）", () => {
  it("非分解订单行菜单 = 详情 / 编辑 / 退款 / 加入物品 / 删除：详情是增量，编辑通道保留", async () => {
    setTxnDb([makeTxn(1, "acc-1", { source_order_no: "JD-9001" })]);
    const wrapper = await mountView();
    await openMenuOnRow(wrapper, 0);
    expect(rowMenuKeys(wrapper)).toEqual([
      "detail",
      "edit",
      "refund",
      "add-item",
      "menu-divider",
      "delete",
    ]);
  });

  it("无订单号的普通行菜单不变（零回归：无详情项）", async () => {
    setTxnDb([makeTxn(1, "acc-1")]);
    const wrapper = await mountView();
    await openMenuOnRow(wrapper, 0);
    expect(rowMenuKeys(wrapper)).toEqual(["edit", "refund", "add-item", "menu-divider", "delete"]);
  });
});

describe("详情订单区（issue #1862）", () => {
  it("非分解订单行详情：订单区呈现行数 · 合计 · 出资构成徽标与各行明细（双断言：命令参数 + 渲染）", async () => {
    setTxnDb([makeTxn(1, "acc-1", { source_order_no: "JD-9001" })]);
    const wrapper = await mountWithOrderSummary();
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "detail");
    expect(shownModal(wrapper), "期望详情弹窗打开").toBeTruthy();

    // 调用事实：以行上订单号查询汇总
    expect(lastInvokeArgs("get_transaction_order_summary")).toEqual({ sourceOrderNo: "JD-9001" });

    // 渲染效果：订单区（行数 · 合计 · 构成徽标 · 明细）
    const text = visibleModalText();
    expect(text).toContain("所属订单");
    expect(text).toContain("JD-9001");
    expect(text).toContain("2 行");
    expect(text).toContain("¥291.6");
    // 出资构成徽标：账户名 + 金额（多账户 2 枚）
    expect(text).toContain("现金");
    expect(text).toContain("¥38.53");
    expect(text).toContain("银行");
    expect(text).toContain("¥253.07");
    await closeShownModal(wrapper);
  });

  it("分解订单行详情 = 出资项列表 + 订单区（两区并存）", async () => {
    setTxnDb(orderRows().slice(0, 1));
    const wrapper = await mountWithOrderSummary();
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "detail");
    const text = visibleModalText();
    expect(text).toContain("出资分解");
    expect(text).toContain("所属订单");
    expect(lastInvokeArgs("get_transaction_order_summary")).toEqual({ sourceOrderNo: "JD-9001" });
    await closeShownModal(wrapper);
  });

  it("无订单号行不查订单汇总命令（未布线即未命中报错兜底不触达）", async () => {
    setTxnDb([makeTxn(1, "acc-1")]);
    const wrapper = await mountWithOrderSummary();
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "edit");
    await flushPromises();
    const calls = mockInvoke.mock.calls;
    expect(calls.filter(([c]) => c === "get_transaction_order_summary").length).toBe(0);
  });
});

describe("投资行详情与订单区（issue #1862：点开任一同单行）", () => {
  it("带单号的 dividend 详情：分红明细与订单区并存（组件级，主列表不呈现投资 kind）", async () => {
    wireInvokeSeam({
      defaults: SHELL_DEFAULTS,
      overrides: {
        ...SHELL_OVERRIDES,
        get_transaction_order_summary: () => Promise.resolve(orderSummary()),
      },
      refreshReferenceStores: true,
    });
    const wrapper = mount(NDialogProvider, {
      slots: {
        default: () =>
          h(DividendDetail, {
            transaction: makeTransaction({
              id: "txn-div",
              kind: "dividend",
              account_id: "acc-1",
              source_order_no: "JD-9001",
            }),
          }),
      },
    });
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("所属订单");
    expect(text).toContain("JD-9001");
    expect(lastInvokeArgs("get_transaction_order_summary")).toEqual({ sourceOrderNo: "JD-9001" });
    wrapper.unmount();
  });
});
