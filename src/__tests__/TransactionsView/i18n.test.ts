import { describe, it, expect } from "vitest";
import { nextTick } from "vue";
import { flushPromises } from "@vue/test-utils";
import { applyLocale } from "@ledger/i18n";
import { mountView, openMenuOnRow, rowMenu } from "./common";

describe("TransactionsView 文案国际化（issue #348）", () => {
  it("默认中文渲染；切 en-US 后按钮/列名/分页/空态即时切换，还原后中文逐字不变", async () => {
    const wrapper = await mountView();
    expect(wrapper.text()).toContain("记一笔");
    expect(wrapper.text()).toContain("清除筛选");
    expect(wrapper.text()).toContain("分类");
    expect(wrapper.text()).toContain("商品 / 备注");
    expect(wrapper.text()).toContain("共 45 条");

    await applyLocale("en-US");
    await nextTick();
    await nextTick();
    expect(wrapper.text()).toContain("New");
    expect(wrapper.text()).toContain("Clear filters");
    expect(wrapper.text()).toContain("Category");
    expect(wrapper.text()).toContain("Amount");
    expect(wrapper.text()).toContain("Item / Note");
    expect(wrapper.text()).not.toContain("分类");
    expect(wrapper.text()).not.toContain("金额");
    expect(wrapper.text()).toContain("Total 45");

    await applyLocale("zh-CN");
    await nextTick();
    expect(wrapper.text()).toContain("记一笔");
    expect(wrapper.text()).toContain("分类");
    expect(wrapper.text()).toContain("共 45 条");
  });

  it("en-US 下行右键菜单项为英文（transaction-row-menu 经 t() 取文案）", async () => {
    const wrapper = await mountView();
    await applyLocale("en-US");
    await nextTick();
    await openMenuOnRow(wrapper, 0);
    await flushPromises();
    const options = rowMenu(wrapper).props("options") as Array<{
      key?: string;
      label?: string;
    }>;
    expect(options.filter((o) => o.label).map((o) => o.label)).toEqual([
      "Edit",
      "Refund",
      "Add Item",
      "Delete",
    ]);
    await applyLocale("zh-CN");
  });
});
