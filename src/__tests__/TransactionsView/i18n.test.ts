import { describe, it, expect } from "vitest";
import { nextTick } from "vue";
import { flushPromises } from "@vue/test-utils";
import { applyLocale } from "@ledger/i18n";
import { mountView, openMenuOnRow, rowMenu } from "./common";

// 交易域文案 i18n（issue #348）：默认语言恒为 zh-CN（测试环境不初始化），
// 切换经 applyLocale（en 文案包懒加载），断言后还原 zh-CN 避免污染单例语言状态。
describe("TransactionsView 文案国际化（issue #348）", () => {
  it("默认中文渲染；切 en-US 后按钮/列名/分页/空态即时切换，还原后中文逐字不变", async () => {
    const wrapper = await mountView();
    // 默认 zh-CN：按钮、列名、分页前缀均为中文
    expect(wrapper.text()).toContain("记一笔");
    expect(wrapper.text()).toContain("清除筛选");
    expect(wrapper.text()).toContain("分类");
    // 展开视图列头「商品 / 备注」（一列两用，issue #1883 / ADR-0138 决策 13）
    expect(wrapper.text()).toContain("商品 / 备注");
    expect(wrapper.text()).toContain("共 45 条");

    await applyLocale("en-US");
    await nextTick();
    await nextTick();
    expect(wrapper.text()).toContain("New");
    expect(wrapper.text()).toContain("Clear filters");
    // 列名随语言重建（computed columns）：中文列名消失、英文列名出现
    expect(wrapper.text()).toContain("Category");
    expect(wrapper.text()).toContain("Amount");
    expect(wrapper.text()).toContain("Item / Note");
    expect(wrapper.text()).not.toContain("分类");
    expect(wrapper.text()).not.toContain("金额");
    // 列头「备注」随切换消失（行内备注文本不受列名影响，不对整页文本断「备注」缺席）
    // 分页前缀随语言切换
    expect(wrapper.text()).toContain("Total 45");

    // 还原 zh-CN：中文逐字不变
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
