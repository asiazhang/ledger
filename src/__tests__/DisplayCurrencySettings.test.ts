import { describe, it, expect } from "vitest";
import { mockInvoke } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { NSelect } from "naive-ui";
import DisplayCurrencySettings from "@/settings/DisplayCurrencySettings.vue";

/**
 * DisplayCurrencySettings 组件测试（issue #858 拆分为设备级偏好；issue #1664 自
 * 「通用」迁入「币种」页签并抽为独立卡片）：展示币种是轻量设置项——localStorage
 * 权威、不触后端、不随同步迁移；作用域是本机表单预选与金额符号提示。
 */

function selects(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAllComponents(NSelect);
}

describe("DisplayCurrencySettings.vue — 展示币种（轻量设备偏好，issue #858 / #1664）", () => {
  it("渲染展示币种卡片与设备偏好提示，不含账本级本位币基准与同步动作", () => {
    const wrapper = mount(DisplayCurrencySettings);
    expect(wrapper.text()).toContain("展示币种");
    expect(wrapper.text()).toContain("不随同步");
    expect(wrapper.text()).not.toContain("本位币基准");
    expect(wrapper.text()).not.toContain("立即同步");
  });

  it("展示币种仍是设备偏好：切换写 localStorage、不触后端命令", async () => {
    const wrapper = mount(DisplayCurrencySettings);
    await flushPromises();
    selects(wrapper)[0].vm.$emit("update:value", "HKD");
    await flushPromises();
    expect(JSON.parse(localStorage.getItem("default_currency") ?? "null")).toBe("HKD");
    expect(mockInvoke.mock.calls.filter(([c]) => c === "set_base_currency")).toHaveLength(0);
  });
});
