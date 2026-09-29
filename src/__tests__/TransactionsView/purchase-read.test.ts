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
/**
 * 交易列表逐购买项呈现（issue #1883 / ADR-0138 决策 13）：带购买项的交易展开为
 * 「每个购买项各占一行」，订单级列（日期/类型/商户/来源/账户/金额/操作）纵向合并
 * 成一格、金额只出现一次订单实付；分类列逐行显示各购买项自己的分类（不合并）；
 * 备注列改名「商品 / 备注」一列两用；操作列订单块内只留一个，购买项行不可交互。
 * 断言对准列表的外部可见行为；删除视图侧展开/合并接线（`:data` 行集投影或列
 * 构造器 rowSpan / 两用渲染）即本文件变红（接线负向条目）。
 */

/** 分类字典（列级覆写）：两个在用根分类，供逐行分类断言（组件测试数据工厂单源）。 */
const purchaseCategories: Category[] = [
  makeCategory({ id: "cat-1", name: "餐饮" }),
  makeCategory({ id: "cat-2", name: "日用品", sort_order: 1 }),
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
    // 订单块首行（承载合并格）右键开订单级菜单：详情入口开放（issue #1884 / ADR-0138
    // 决策 15），编辑关闭（编辑表单未支持购买项、全字段替换会清空子行）
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
    // 3 件商品的订单占前 3 行：第 1 / 2 行 = 块内行（行线抹掉），第 3 行 = 块尾（留整宽行线）
    expect(rows[0].classes()).toContain("purchase-block-row");
    expect(rows[1].classes()).toContain("purchase-block-row");
    expect(rows[2].classes()).toContain("purchase-block-row");
    expect(rows[0].classes()).not.toContain("purchase-block-last");
    expect(rows[1].classes()).not.toContain("purchase-block-last");
    expect(rows[2].classes()).toContain("purchase-block-last");
    // 无购买项的交易行不带订单块类（样式零影响）
    expect(rows[3].classes()).not.toContain("purchase-block-row");
    expect(rows[3].classes()).not.toContain("purchase-block-last");
    // global.css 规则的结构前提（删掉合并接线即红）：合并格只在块首行，块内行只剩逐行的
    // 「分类 / 商品·备注」两格——那条短行线正是把一单切碎的东西
    expect(rows[0].findAll(".purchase-merged-cell").length).toBeGreaterThan(0);
    expect(rows[1].findAll(".purchase-merged-cell").length).toBe(0);
  });
  it("订单级合并格底色：悬停只点亮被悬停那一单，别的订单的合并格不留 hover 底色", async () => {
    // 底色走 naive 的 `-td--hover`（jsdom 不解析 var() 取不到计算色，故以该修饰类为可观察代理）：
    // 行键取不到时它会被贴到全表每个 rowspan 格上常亮，合并格底色与普通行不同、
    // 且与行线色接近 → 相邻两单连成一条浅色带、块间分隔线被淹没（row-key 缺失的真实现象）
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
    await rows[3].trigger("mouseenter"); // 悬停第二单的续行
    const hover = "n-data-table-td--hover";
    // 第二单（行 2/3）的合并格被点亮：悬停任一购买项行都点亮整单的订单级格
    expect(rows[2].findAll("td")[0].classes()).toContain(hover);
    // 第一单（行 0/1）不受影响：底色回到与普通行一致
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
    // 9 笔普通交易 + 1 笔三件商品订单 = 10 笔交易 → 展开 12 个视觉行
    const plain = Array.from({ length: 9 }, (_, i) => makeTxn(i + 10, "acc-1"));
    const wrapper = await mountWithPurchases([...plain, orderRow()]);
    const pagination = tablePagination(wrapper);
    expect(pagination.itemCount).toBe(10);
    expect(bodyRows(wrapper).length).toBe(12);
    expect(wrapper.text()).toContain("共 10 条");
  });
});
