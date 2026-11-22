import { describe, it, expect, afterEach, vi } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { createMemoryHistory, createRouter } from "vue-router";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import App from "@/App.vue";
import { routes } from "@/router";
import { useSidebarOrderStore } from "@/stores/sidebar-order";
import { hasOpenOverlay } from "@ledger/ui-kit/overlayRegistry";
import {
  openMobileDrawer,
  drawerMenuItemTexts,
  drawerMoreLinkTexts,
  drawerGroupTitles,
  findDrawerItem,
} from "@ledger/test-support/mobile-nav";

async function mountApp() {
  wireInvokeSeam({ defaults: { get_boot_status: { phase: "ready", error_code: null } } });
  const router = createRouter({ history: createMemoryHistory(), routes });
  await router.push("/dashboard");
  await router.isReady();
  const wrapper = mount(App, { global: { plugins: [router], stubs: { RouterView: true } } });
  await flushPromises();
  return { wrapper, router };
}

async function preloadRouteView(name: string) {
  const record = routes.find((r) => r.name === name);
  if (!record || typeof record.component !== "function") {
    throw new Error(`路由表缺少懒加载路由 ${name}`);
  }
  await (record.component as () => Promise<unknown>)();
}

describe("App 壳窗口分级分支（issue #842，假 matchMedia 换档）", () => {
  afterEach(() => {
    useSidebarOrderStore().resetSidebarOrder();
  });

  it("≥840 桌面档零变化：侧栏在、顶栏与抽屉不在，弹层注册表为空；键位提示照渲染", async () => {
    const { wrapper } = await mountApp();
    expect(wrapper.find(".n-layout-sider").exists()).toBe(true);
    expect(wrapper.find(".mobile-top-bar").exists()).toBe(false);
    expect(document.body.querySelector(".n-drawer")).toBeNull();
    expect(hasOpenOverlay()).toBe(false);
    const siderText = wrapper.find(".n-layout-sider").text();
    expect(siderText).toContain("⌃1");
    expect(siderText).toContain("⌃,");
  });

  it("缩窗跨断点（1280 → 839）：侧栏卸载、移动顶栏即现（视图名随路由）", async () => {
    const { wrapper } = await mountApp();
    expect(wrapper.find(".n-layout-sider").exists()).toBe(true);
    setFakeMedia({ width: 839 });
    await flushPromises();
    expect(wrapper.find(".n-layout-sider").exists()).toBe(false);
    expect(wrapper.find(".mobile-top-bar").exists()).toBe(true);
    expect(wrapper.find(".mobile-top-bar-title").text()).toBe("概览");
  });

  it("<840 抽屉可达全部视图：固定项 + 各组主项 + 出厂收纳组「更多」链接；洞察组（空清单）无链接", async () => {
    setFakeMedia({ width: 839 });
    const { wrapper } = await mountApp();
    await openMobileDrawer(wrapper);
    expect(hasOpenOverlay()).toBe(true);
    const items = drawerMenuItemTexts().join("|");
    for (const label of [
      "概览",
      "交易",
      "账户",
      "预算",
      "投资",
      "物品",
      "报表",
      "搜索",
      "AI",
      "设置",
    ]) {
      expect(items, `抽屉应含 ${label}`).toContain(label);
    }
    expect(drawerMoreLinkTexts()).toEqual(["更多", "更多"]);
    expect(drawerGroupTitles()).toEqual(["记账", "资产", "洞察"]);
  });

  it("移动档不渲染键位带（ADR-0088 关联 ADR-0065）：抽屉菜单零 ⌘/⌃ 提示，与桌面档同源不同饰", async () => {
    setFakeMedia({ width: 839 });
    const { wrapper } = await mountApp();
    await openMobileDrawer(wrapper);
    for (const text of drawerMenuItemTexts()) {
      expect(text, `抽屉菜单项不应含键位提示：${text}`).not.toMatch(/[⌘⌃]/);
    }
    expect(drawerMenuItemTexts()).toHaveLength(11);
  });

  it("ViewState 组内序在抽屉中正确渲染：存量自定义序经解析防御后按序呈现", async () => {
    localStorage.setItem(
      "view_state:sidebar_order",
      JSON.stringify({ bookkeeping: ["budget", "transactions", "accounts"] }),
    );
    setFakeMedia({ width: 839 });
    const { wrapper } = await mountApp();
    await openMobileDrawer(wrapper);
    const texts = drawerMenuItemTexts();
    const indexOf = (label: string) => texts.findIndex((t) => t.includes(label));
    expect(indexOf("预算")).toBeGreaterThan(-1);
    expect(indexOf("预算")).toBeLessThan(indexOf("交易"));
    expect(indexOf("交易")).toBeLessThan(indexOf("账户"));
  });

  it("收纳清单写路径语义零改动：移入后抽屉即现该组「更多」链接，移回即消失", async () => {
    setFakeMedia({ width: 839 });
    const { wrapper } = await mountApp();
    await openMobileDrawer(wrapper);
    expect(drawerMoreLinkTexts().length).toBe(2);
    const store = useSidebarOrderStore();
    store.applyMoveIntoMore("search");
    await flushPromises();
    expect(drawerMoreLinkTexts().length).toBe(3);
    store.applyMoveBackToSidebar("search");
    await flushPromises();
    expect(drawerMoreLinkTexts().length).toBe(2);
  });

  it("抽屉菜单项导航：路由切换 + 抽屉关闭（注册表撤销上报）", async () => {
    setFakeMedia({ width: 839 });
    const { wrapper, router } = await mountApp();
    await preloadRouteView("transactions");
    await openMobileDrawer(wrapper);
    findDrawerItem("交易").click();
    await vi.waitFor(() => {
      expect(router.currentRoute.value.name).toBe("transactions");
    });
    await flushPromises();
    expect(hasOpenOverlay()).toBe(false);
    expect(wrapper.find(".mobile-top-bar-title").text()).toBe("交易");
  });

  it("抽屉「更多」链接：跳转该组聚合页 + 抽屉关闭（导航即关闭统一收口）", async () => {
    setFakeMedia({ width: 839 });
    const { wrapper, router } = await mountApp();
    await preloadRouteView("bookkeeping-more");
    await openMobileDrawer(wrapper);
    (document.body.querySelector(".n-drawer .group-more-link") as HTMLElement).click();
    await vi.waitFor(() => {
      expect(router.currentRoute.value.name).toBe("bookkeeping-more");
    });
    await flushPromises();
    expect(hasOpenOverlay()).toBe(false);
    expect(wrapper.find(".mobile-top-bar-title").text()).toBe("记账 · 更多");
  });

  it("开着抽屉跨断点回桌面（839 → 900）：移动壳卸载、注册表兜底撤销、侧栏回归", async () => {
    setFakeMedia({ width: 839 });
    const { wrapper } = await mountApp();
    await openMobileDrawer(wrapper);
    expect(hasOpenOverlay()).toBe(true);
    setFakeMedia({ width: 900 });
    await flushPromises();
    expect(wrapper.find(".mobile-top-bar").exists()).toBe(false);
    expect(wrapper.find(".n-layout-sider").exists()).toBe(true);
    expect(hasOpenOverlay()).toBe(false);
  });
});
