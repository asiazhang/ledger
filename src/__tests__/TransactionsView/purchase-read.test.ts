import { describe, expect, it } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import type { Category, Transaction } from "@ledger/types";
import { visibleModalText } from "@ledger/test-support/dom";
import { makeCategory, makeFunding, makePurchase } from "../factories";
import {
  bodyRows,
  makeTxn,
  mountView,
  openMenuOnRow,
  rowMenu,
  rowMenuKeys,
  selectRowMenu,
  setTxnDb,
  closeShownModal,
  shownModal,
  tablePagination,
  SHELL_DEFAULTS,
  SHELL_OVERRIDES,
} from "./common";

const purchaseCategories: Category[] = [
  makeCategory({ id: "cat-1", name: "餐饮" }),
  makeCategory({ id: "cat-2", name: "日用品", sort_order: 1 }),
];

function orderRow(): Transaction {
  return makeTxn(1, "acc-1", {
    kind: "expense",
    amount_cents: 9990,
    amount_native_cents: 9990,
    purchases: [
      makePurchase({ name: "猫粮", quantity: 1, category_id: "cat-1" }),
      makePurchase({ name: "洗衣液", quantity: 2, category_id: "cat-2" }),
      makePurchase({ name: "纸巾", quantity: 1 }),
    ],
  });
}

async function mountWithPurchases(rows: Transaction[]) {
  const seam = wireInvokeSeam({
    defaults: SHELL_DEFAULTS,
    overrides: { ...SHELL_OVERRIDES, list_categories: () => purchaseCategories },
    refreshReferenceStores: true,
  });
  await seam.ready;
  setTxnDb(rows);
  return mountView();
}

describe("交易列表逐购买项呈现（issue #1883 / ADR-0138 决策 13）", () => {
  it("3 件商品的订单在列表里占 3 行，商品名按对账单顺序逐行显示", async () => {
    const wrapper = await mountWithPurchases([orderRow()]);
    const rows = bodyRows(wrapper);
    expect(rows.length).toBe(3);
    expect(rows[0].text()).toContain("猫粮");
    expect(rows[1].text()).toContain("洗衣液");
    expect(rows[2].text()).toContain("纸巾");
  });

  it("分类列逐行显示各购买项自己的分类，不合并（一单内分类可以不同）", async () => {
    const wrapper = await mountWithPurchases([orderRow()]);
    const rows = bodyRows(wrapper);
    expect(rows[0].text()).toContain("餐饮");
    expect(rows[1].text()).toContain("日用品");
    expect(rows[2].text()).not.toContain("餐饮");
    expect(rows[2].text()).not.toContain("日用品");
  });

  it("订单级列合并：日期与金额在订单块内只出现一次，操作按钮只一个", async () => {
    const wrapper = await mountWithPurchases([orderRow()]);
    const tbodyText = wrapper.find(".n-data-table-tbody").text();
    expect(tbodyText.split("2026-01-01").length - 1).toBe(1);
    expect(tbodyText.split("¥99.9").length - 1).toBe(1);
    expect(wrapper.findAll(".row-actions-btn").length).toBe(1);
  });

  it("备注列一列两用：有清单行显示「商品名 + 共 N 件」，无清单行仍是备注（省略号 + 复制按钮）", async () => {
    const wrapper = await mountWithPurchases([
      orderRow(),
      makeTxn(2, "acc-1", { note: "手工备注" }),
    ]);
    const rows = bodyRows(wrapper);
    expect(rows[0].text()).toContain("共 1 件");
    expect(rows[1].text()).toContain("共 2 件");
    expect(rows[0].find(".note-copy-btn").exists()).toBe(false);
    expect(rows[3].text()).toContain("手工备注");
    expect(rows[3].find(".note-copy-btn").exists()).toBe(true);
    expect(wrapper.find(".n-data-table-thead").text()).toContain("商品 / 备注");
  });

  it("购买项行不可交互：续行右键无菜单，订单块首行右键仍开订单级菜单", async () => {
    const wrapper = await mountWithPurchases([orderRow(), makeTxn(2, "acc-1")]);
    await openMenuOnRow(wrapper, 1);
    expect(rowMenu(wrapper)).toBeUndefined();
    await openMenuOnRow(wrapper, 0);
    expect(rowMenuKeys(wrapper)).toEqual([
      "detail",
      "refund",
      "add-item",
      "menu-divider",
      "delete",
    ]);
  });

  it("订单块行类：块内行 / 块尾行分标，普通行不带类（块边界由块尾整宽行线承担）", async () => {
    const wrapper = await mountWithPurchases([orderRow(), makeTxn(2, "acc-1")]);
    const rows = bodyRows(wrapper);
    expect(rows[0].classes()).toContain("purchase-block-row");
    expect(rows[1].classes()).toContain("purchase-block-row");
    expect(rows[2].classes()).toContain("purchase-block-row");
    expect(rows[0].classes()).not.toContain("purchase-block-last");
    expect(rows[1].classes()).not.toContain("purchase-block-last");
    expect(rows[2].classes()).toContain("purchase-block-last");
    expect(rows[3].classes()).not.toContain("purchase-block-row");
    expect(rows[3].classes()).not.toContain("purchase-block-last");
    expect(rows[0].findAll(".purchase-merged-cell").length).toBeGreaterThan(0);
    expect(rows[1].findAll(".purchase-merged-cell").length).toBe(0);
  });
  it("订单级合并格底色：悬停只点亮被悬停那一单，别的订单的合并格不留 hover 底色", async () => {
    const order = (id: number, a: string, b: string) =>
      makeTxn(id, "acc-1", {
        kind: "expense",
        amount_cents: 9990,
        amount_native_cents: 9990,
        purchases: [
          makePurchase({ name: a, quantity: 3, category_id: "cat-1" }),
          makePurchase({ name: b, quantity: 1, category_id: "cat-2" }),
        ],
      });
    const wrapper = await mountWithPurchases([
      order(1, "达喜", "蓓安适"),
      order(2, "达喜", "洁丽雅"),
    ]);
    const rows = bodyRows(wrapper);
    await rows[3].trigger("mouseenter");
    const hover = "n-data-table-td--hover";
    expect(rows[2].findAll("td")[0].classes()).toContain(hover);
    expect(rows[0].findAll("td")[0].classes()).not.toContain(hover);
    expect(rows[0].findAll("td")[7].classes()).not.toContain(hover);
  });

  it("订单块行类：单件商品订单首行即末行（块尾标记使块内规则不命中，行线保持原样）", async () => {
    const wrapper = await mountWithPurchases([
      makeTxn(1, "acc-1", { purchases: [makePurchase({ name: "猫粮", quantity: 1 })] }),
    ]);
    const row = bodyRows(wrapper)[0];
    expect(row.classes()).toContain("purchase-block-row");
    expect(row.classes()).toContain("purchase-block-last");
  });

  it("购买项行详情弹窗：清单列出名称 / 件数 / 分类（列表侧商品项渲染与文案单源）", async () => {
    const wrapper = await mountWithPurchases([orderRow()]);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "detail");
    expect(shownModal(wrapper), "期望详情弹窗打开").toBeTruthy();
    const text = visibleModalText();
    expect(text).toContain("购买项");
    expect(text).toContain("猫粮");
    expect(text).toContain("洗衣液");
    expect(text).toContain("纸巾");
    expect(text).toContain("共 2 件");
    expect(text).toContain("餐饮");
    expect(text).toContain("日用品");
    await closeShownModal(wrapper);
  });

  it("购买项 + 订单号行详情：购买项清单与所属订单区并存", async () => {
    setTxnDb([makeTxn(1, "acc-1", { ...orderRow(), source_order_no: "JD-9001" })]);
    wireInvokeSeam({
      defaults: SHELL_DEFAULTS,
      overrides: {
        ...SHELL_OVERRIDES,
        get_transaction_order_summary: () =>
          Promise.resolve({
            source_order_no: "JD-9001",
            row_count: 1,
            total_amount_cents: 9990,
            currency_code: "CNY",
            accounts: [],
            items: [],
          }),
      },
      refreshReferenceStores: true,
    });
    const wrapper = await mountView();
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "detail");
    const text = visibleModalText();
    expect(text).toContain("购买项");
    expect(text).toContain("猫粮");
    expect(text).toContain("所属订单");
    expect(text).toContain("JD-9001");
    await closeShownModal(wrapper);
  });

  it("分解 + 购买项行详情：出资分解与购买项清单同层并陈（ADR-0138 决策 15 对称）", async () => {
    setTxnDb([
      makeTxn(1, null, {
        ...orderRow(),
        fundings: [makeFunding({ account_id: "acc-1", amount_cents: 9990 })],
      }),
    ]);
    const wrapper = await mountView();
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "detail");
    const text = visibleModalText();
    expect(text).toContain("出资分解");
    expect(text).toContain("¥99.9");
    expect(text).toContain("购买项");
    expect(text).toContain("猫粮");
    await closeShownModal(wrapper);
  });

  it("分页与「共 N 条」按交易计：一页视觉行数可多于页大小", async () => {
    const plain = Array.from({ length: 9 }, (_, i) => makeTxn(i + 10, "acc-1"));
    const wrapper = await mountWithPurchases([...plain, orderRow()]);
    const pagination = tablePagination(wrapper);
    expect(pagination.itemCount).toBe(10);
    expect(bodyRows(wrapper).length).toBe(12);
    expect(wrapper.text()).toContain("共 10 条");
  });
});
