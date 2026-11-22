import { describe, it, expect, vi, beforeEach } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import AccountLink from "@/accounts/AccountLink.vue";
import { useAppStore } from "@/stores/app";

const pushMock = vi.fn();
vi.mock("vue-router", () => ({
  useRouter: () => ({ push: pushMock }),
}));

beforeEach(async () => {
  pushMock.mockReset();
  await wireInvokeSeam({ refreshReferenceStores: true }).ready;
});

describe("AccountLink 账户名下钻（issue #97/#99）", () => {
  it("渲染为真实 button（键盘可达）并带 title 提示", async () => {
    const wrapper = mount(AccountLink, { props: { accountId: "acc-1" } });
    await flushPromises();
    const btn = wrapper.find("button.account-link");
    expect(btn.exists()).toBe(true);
    expect(btn.attributes("title")).toBe("查看该账户的交易");
    expect(btn.text()).toBe("现金");
  });

  it("默认态使用主题强调色（琥珀）", async () => {
    const wrapper = mount(AccountLink, { props: { accountId: "acc-1" } });
    await flushPromises();
    const btn = wrapper.find("button");
    expect(btn.attributes("style")).toContain("rgb(245, 158, 11)");
  });

  it("暗色 hover 变量注入亮琥珀（#FBBF24；自定义属性不经 jsdom 颜色归一化）", async () => {
    useAppStore().setTheme("dark");
    const wrapper = mount(AccountLink, { props: { accountId: "acc-1" } });
    await flushPromises();
    expect(wrapper.find("button").attributes("style")).toContain("--accent-hover: #FBBF24");
  });

  it("亮色主题：强调色切同色相加深版（#B45309 / hover #92400E）", async () => {
    useAppStore().setTheme("light");
    const wrapper = mount(AccountLink, { props: { accountId: "acc-1" } });
    await flushPromises();
    const style = wrapper.find("button").attributes("style");
    expect(style).toContain("rgb(180, 83, 9)");
    expect(style).toContain("--accent-hover: #92400E");
  });

  it("点击跳转 /transactions?account=<id>", async () => {
    const wrapper = mount(AccountLink, { props: { accountId: "acc-1" } });
    await flushPromises();
    await wrapper.find("button").trigger("click");
    expect(pushMock).toHaveBeenCalledWith({ name: "transactions", query: { account: "acc-1" } });
  });

  it("账户不在参考数据中（黑洞/隐藏账户）渲染纯文本「-」，不可点击、无跳转", async () => {
    const wrapper = mount(AccountLink, { props: { accountId: "ghost-acc" } });
    await flushPromises();
    expect(wrapper.find("button").exists()).toBe(false);
    const ph = wrapper.find("span.account-placeholder");
    expect(ph.exists()).toBe(true);
    expect(ph.text()).toBe("-");
    expect(ph.attributes("title")).toBeUndefined();
    await ph.trigger("click");
    expect(pushMock).not.toHaveBeenCalled();
  });

  it("外部传入的布局样式透传到根按钮（转账行内容宽度/左对齐依赖，issue #99）", async () => {
    const wrapper = mount(AccountLink, {
      props: { accountId: "acc-1" },
      attrs: { style: "flex: 0 1 auto; min-width: 0; text-align: left;" },
    });
    await flushPromises();
    const btn = wrapper.find("button");
    const style = (btn.element as HTMLElement).style;
    expect(style.flexGrow).toBe("0");
    expect(style.flexShrink).toBe("1");
    expect(style.minWidth).toBe("0px");
    expect(style.textAlign).toBe("left");
    expect(btn.attributes("style")).toContain("rgb(245, 158, 11)");
  });
});
