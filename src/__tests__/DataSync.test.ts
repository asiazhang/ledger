import { describe, it, expect, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import SettingsView from "@/views/SettingsView.vue";
import CategoryManager from "@/categories/CategoryManager.vue";

beforeEach(async () => {
  await wireInvokeSeam({ refreshReferenceStores: true }).ready;
});

describe("SettingsView 不含同步入口（issue #111）", () => {
  it("设置页不渲染任何同步入口与同步文案", async () => {
    const wrapper = mount(SettingsView);
    expect(wrapper.html()).not.toContain("开始同步");
    expect(wrapper.html()).not.toContain("全量同步");
    expect(wrapper.html()).not.toContain("东方财富");
    expect(wrapper.html()).not.toContain("数据管理");
  });

  it("包含 CategoryManager 组件（「分类」Tab 内）", async () => {
    const wrapper = mount(SettingsView);
    const tab = wrapper.findAll(".n-tabs-tab").find((t) => t.text() === "分类");
    expect(tab).toBeTruthy();
    await tab!.trigger("click");
    await nextTick();
    expect(wrapper.findComponent(CategoryManager).exists()).toBe(true);
  });
});
