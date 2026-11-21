import { describe, expect, it } from "vitest";
import type { Transaction } from "@ledger/types";
import { visibleModalText } from "@ledger/test-support/dom";
import { makeFunding } from "../factories";
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
} from "./common";

function comboRow(): Transaction {
  return makeTxn(1, null, {
    kind: "expense",
    note: "组合支付",
    amount_cents: 28160,
    amount_native_cents: 28160,
    fundings: [
      makeFunding({ account_id: "acc-1", amount_cents: 2853, label: "罐装" }),
      makeFunding({ account_id: "acc-2", amount_cents: 25307, label: "固体饼干" }),
    ],
  });
}

function sameAccountRow(): Transaction {
  return makeTxn(2, null, {
    kind: "expense",
    note: "同账户双笔",
    amount_cents: 450000,
    amount_native_cents: 450000,
    fundings: [
      makeFunding({ account_id: "acc-1", amount_cents: 50000, label: "预售定金" }),
      makeFunding({ account_id: "acc-1", amount_cents: 400000, label: "尾款" }),
    ],
  });
}

function derivedRefundRow(): Transaction {
  return makeTxn(3, null, {
    kind: "refund",
    note: "整单退款",
    amount_cents: 28160,
    amount_native_cents: 28160,
    refund_of_transaction_id: "txn-001",
    fundings: [
      makeFunding({ account_id: "acc-1", amount_cents: 2853, label: "罐装", derived: true }),
      makeFunding({ account_id: "acc-2", amount_cents: 25307, label: "固体饼干", derived: true }),
    ],
  });
}

function plainRow(): Transaction {
  return makeTxn(4, "acc-1", { kind: "expense", note: "普通支出" });
}

describe("交易列表分解行账户列（issue #1861 / ADR-0138 决策 8）", () => {
  it("组合支付行显示「首账户 等 2 账户」，只列首条出资账户，不列其余账户", async () => {
    setTxnDb([comboRow(), plainRow()]);
    const wrapper = await mountView();
    const row = wrapper.find(".n-data-table-tbody .n-data-table-tr");
    expect(row.text()).toContain("现金");
    expect(row.text()).toContain("等 2 账户");
    expect(row.text()).not.toContain("银行");
  });

  it("同账户双笔退化「同账户 2 笔」（不呈现「等 N 账户」）", async () => {
    setTxnDb([sameAccountRow(), plainRow()]);
    const wrapper = await mountView();
    const row = wrapper.find(".n-data-table-tbody .n-data-table-tr");
    expect(row.text()).toContain("现金");
    expect(row.text()).toContain("同账户 2 笔");
    expect(row.text()).not.toContain("等 2 账户");
  });

  it("退款派生分解行带「按比例自动分解」标注，账户列同「首账户 + 等 N 账户」规则", async () => {
    setTxnDb([derivedRefundRow(), plainRow()]);
    const wrapper = await mountView();
    const row = wrapper.find(".n-data-table-tbody .n-data-table-tr");
    expect(row.text()).toContain("按比例自动分解");
    expect(row.text()).toContain("现金");
    expect(row.text()).toContain("等 2 账户");
  });

  it("单出资行零回归：账户列只显示主账户名，无任何分解标注", async () => {
    setTxnDb([plainRow()]);
    const wrapper = await mountView();
    const row = wrapper.find(".n-data-table-tbody .n-data-table-tr");
    expect(row.text()).toContain("现金");
    expect(row.text()).not.toContain("等 2 账户");
    expect(row.text()).not.toContain("同账户");
    expect(row.text()).not.toContain("按比例自动分解");
  });

  it("移动档卡片共用同一渲染单源：分解标注与派生标注照常呈现", async () => {
    setTxnDb([comboRow(), derivedRefundRow(), plainRow()]);
    const wrapper = await mountMobile();
    const list = cards(wrapper);
    expect(list.length).toBe(3);
    expect(list[0].text()).toContain("等 2 账户");
    expect(list[1].text()).toContain("按比例自动分解");
    expect(list[2].text()).not.toContain("等 2 账户");
  });
});

describe("分解行菜单与只读详情（issue #1861）", () => {
  it("分解 expense 行菜单：详情 / 加入物品 / 分隔线 / 删除（编辑/退款表单未支持分解，不开放）", async () => {
    setTxnDb([comboRow(), plainRow()]);
    const wrapper = await mountView();
    await openMenuOnRow(wrapper, 0);
    expect(rowMenuKeys(wrapper)).toEqual(["detail", "add-item", "menu-divider", "delete"]);
    await openMenuOnRow(wrapper, 1);
    expect(rowMenuKeys(wrapper)).toEqual(["edit", "refund", "add-item", "menu-divider", "delete"]);
  });

  it("详情展示出资项列表（账户名 / 扣款标签徽标 / 金额）与 Σ 合计行", async () => {
    setTxnDb([comboRow()]);
    const wrapper = await mountView();
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "detail");
    const modal = shownModal(wrapper);
    expect(modal, "期望详情弹窗打开").toBeTruthy();
    const text = visibleModalText();
    expect(text).toContain("出资分解");
    expect(text).toContain("现金");
    expect(text).toContain("银行");
    expect(text).toContain("罐装");
    expect(text).toContain("固体饼干");
    expect(text).toContain("¥28.53");
    expect(text).toContain("¥253.07");
    expect(text).toContain("¥281.6");
    await closeShownModal(wrapper);
  });

  it("退款派生分解详情同样呈现出资项，并带「按比例自动分解」标注", async () => {
    setTxnDb([derivedRefundRow()]);
    const wrapper = await mountView();
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "detail");
    const modal = shownModal(wrapper);
    expect(modal, "期望详情弹窗打开").toBeTruthy();
    const text = visibleModalText();
    expect(text).toContain("按比例自动分解");
    expect(text).toContain("罐装");
    expect(text).toContain("¥253.07");
    await closeShownModal(wrapper);
  });

  it("移动档整卡点击 = 分解行进只读详情（不进编辑表单）", async () => {
    setTxnDb([comboRow()]);
    const wrapper = await mountMobile();
    await cards(wrapper)[0].trigger("click");
    const modal = shownModal(wrapper);
    expect(modal, "期望详情弹窗打开").toBeTruthy();
    expect(visibleModalText()).toContain("出资分解");
    await closeShownModal(wrapper);
  });
});
