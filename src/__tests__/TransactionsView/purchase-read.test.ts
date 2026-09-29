import { describe, expect, it } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import type { Category, Transaction } from "@ledger/types";
import { makePurchase } from "../factories";
import {
  bodyRows,
  makeTxn,
  mountView,
  openMenuOnRow,
  rowMenu,
  rowMenuKeys,
  setTxnDb,
  tablePagination,
  SHELL_DEFAULTS,
  SHELL_OVERRIDES,
} from "./common";

/**
 * 交易列表逐购买项呈现（issue #1883 / ADR-0138 决策 13）：带购买项的交易展开为
 * 「每个购买项各占一行」，订单级列（日期/类型/商户/来源/账户/金额/操作）纵向合并
 * 成一格、金额只出现一次订单实付；分类列逐行显示各购买项自己的分类（不合并）；
 * 备注列改名「商品 / 备注」一列两用；操作列订单块内只留一个，购买项行不可交互。
 * 断言对准列表的外部可见行为；删除视图侧展开/合并接线（`:data` 行集投影或列
 * 构造器 rowSpan / 两用渲染）即本文件变红（接线负向条目）。
 */

/** 分类字典（列级覆写）：两个在用根分类，供逐行分类断言。 */
const purchaseCategories: Category[] = [
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
  {
    id: "cat-2",
    name: "日用品",
    kind: "expense",
    parent_id: null,
    icon: null,
    sort_order: 1,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
  },
];

/** 三件商品订单（京东一单多件形态）：猫粮 / 洗衣液 / 纸巾，数组顺序 = 对账单顺序。 */
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

/** 挂载视图：覆写分类字典（逐行分类断言需要两个在用分类）。 */
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
    // 无分类购买项回退占位符，不落任何别行的分类
    expect(rows[2].text()).not.toContain("餐饮");
    expect(rows[2].text()).not.toContain("日用品");
  });

  it("订单级列合并：日期与金额在订单块内只出现一次，操作按钮只一个", async () => {
    const wrapper = await mountWithPurchases([orderRow()]);
    const tbodyText = wrapper.find(".n-data-table-tbody").text();
    // 三行同单：合并前日期/金额各出现 3 次，合并后各 1 次（金额 = 订单实付只显示一次）
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
    // 有清单的行：商品名 + 件数，无复制按钮
    expect(rows[0].text()).toContain("共 1 件");
    expect(rows[1].text()).toContain("共 2 件");
    expect(rows[0].find(".note-copy-btn").exists()).toBe(false);
    // 无清单的行：原备注渲染零变化（文本 + 复制按钮）
    expect(rows[3].text()).toContain("手工备注");
    expect(rows[3].find(".note-copy-btn").exists()).toBe(true);
    // 列头改名（展开视图整列生效）
    expect(wrapper.find(".n-data-table-thead").text()).toContain("商品 / 备注");
  });

  it("购买项行不可交互：续行右键无菜单，订单块首行右键仍开订单级菜单", async () => {
    const wrapper = await mountWithPurchases([orderRow(), makeTxn(2, "acc-1")]);
    // 续行（第 2 个购买项行）右键不开菜单——行 = 购买项不是可操作对象
    await openMenuOnRow(wrapper, 1);
    expect(rowMenu(wrapper)).toBeUndefined();
    // 订单块首行（承载合并格）右键开订单级菜单，菜单形状零变化
    await openMenuOnRow(wrapper, 0);
    expect(rowMenuKeys(wrapper)).toEqual(["edit", "refund", "add-item", "menu-divider", "delete"]);
  });

  it("分页与「共 N 条」按交易计：一页视觉行数可多于页大小", async () => {
    // 9 笔普通交易 + 1 笔三件商品订单 = 10 笔交易 → 展开 12 个视觉行
    const plain = Array.from({ length: 9 }, (_, i) => makeTxn(i + 10, "acc-1"));
    const wrapper = await mountWithPurchases([...plain, orderRow()]);
    const pagination = tablePagination(wrapper);
    expect(pagination.itemCount).toBe(10);
    expect(bodyRows(wrapper).length).toBe(12);
    expect(wrapper.text()).toContain("共 10 条");
  });
});
