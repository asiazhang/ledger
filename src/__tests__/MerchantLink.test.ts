import { describe, it, expect, vi, beforeEach } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import MerchantLink from "@/merchants/MerchantLink.vue";
import { useAppStore } from "@/stores/app";

// MerchantLink 经 useRouter 跳转（AccountLink 同款 pushMock 断言先例）
const pushMock = vi.fn();
vi.mock("vue-router", () => ({
  useRouter: () => ({ push: pushMock }),
}));

beforeEach(async () => {
  pushMock.mockReset();
  // 参考 store 预载走接缝 opt-in 参数（list_merchants 由桩层规范夹具兜底，
  // 含断言消费的 mch-1「京东」行）。
  await wireInvokeSeam({ refreshReferenceStores: true }).ready;
});

describe("MerchantLink 商户名下钻（issue #191）强调色（issue #1268）", () => {
  it("暗色主题（默认）：主题强调色琥珀 + hover 变量亮琥珀", async () => {
    useAppStore().setTheme("dark");
    const wrapper = mount(MerchantLink, { props: { merchantId: "mch-1" } });
    await flushPromises();
    const style = wrapper.find("button").attributes("style");
    expect(style).toContain("rgb(245, 158, 11)");
    expect(style).toContain("--accent-hover: #FBBF24");
  });

  it("亮色主题：强调色切同色相加深版（#B45309 / hover #92400E）", async () => {
    useAppStore().setTheme("light");
    const wrapper = mount(MerchantLink, { props: { merchantId: "mch-1" } });
    await flushPromises();
    const style = wrapper.find("button").attributes("style");
    expect(style).toContain("rgb(180, 83, 9)");
    expect(style).toContain("--accent-hover: #92400E");
  });

  it("未知商户 id 渲染纯文本「-」，无按钮、无强调色", async () => {
    const wrapper = mount(MerchantLink, { props: { merchantId: "ghost-mch" } });
    await flushPromises();
    expect(wrapper.find("button").exists()).toBe(false);
    expect(wrapper.find("span").text()).toBe("-");
  });
});
