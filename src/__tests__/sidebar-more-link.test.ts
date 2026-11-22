import { describe, it, expect, afterEach } from "vitest";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { createMemoryHistory, createRouter } from "vue-router";
import App from "@/App.vue";
import { routes } from "@/router";
import { useSidebarOrderStore, GROUP_CONTAINMENT_SEEDS } from "@/stores/sidebar-order";

async function mountApp() {
  wireInvokeSeam({ defaults: { get_boot_status: { phase: "ready", error_code: null } } });
  const r = createRouter({ history: createMemoryHistory(), routes });
  await r.push("/dashboard");
  await r.isReady();
  const wrapper = mount(App, { global: { plugins: [r], stubs: { RouterView: true } } });
  await flushPromises();
  return wrapper;
}

function linkCount(wrapper: { findAll: (s: string) => unknown[] }): number {
  return wrapper.findAll(".group-more-link").length;
}

describe("侧栏组标题行「更多」链接显隐渲染（issue #475 / ADR-0063 决策 1：清单非空才渲染，移回即消失）", () => {
  afterEach(() => {
    useSidebarOrderStore().resetSidebarOrder();
  });

  it("出厂态：记账（定时/商户）与资产（保单/实物资产）两链接渲染，洞察（空清单）无链接", async () => {
    const wrapper = await mountApp();
    const seededGroups = Object.values(GROUP_CONTAINMENT_SEEDS).filter((s) => s.length > 0).length;
    expect(linkCount(wrapper)).toBe(seededGroups);
  });

  it("移入空组（洞察）后链接即现；移回该组最后一个收纳成员后链接消失（渲染条件失效）", async () => {
    const wrapper = await mountApp();
    const store = useSidebarOrderStore();
    store.applyMoveIntoMore("reports");
    await flushPromises();
    expect(linkCount(wrapper)).toBe(3);
    store.applyMoveIntoMore("search");
    store.applyMoveBackToSidebar("reports");
    await flushPromises();
    expect(linkCount(wrapper)).toBe(3);
    store.applyMoveBackToSidebar("search");
    await flushPromises();
    expect(linkCount(wrapper)).toBe(2);
  });

  it("双向随动不串组：洞察链接随成员移回消失，出厂满员组的移回拒写（≤3 硬上限）不影响任何链接", async () => {
    const wrapper = await mountApp();
    const store = useSidebarOrderStore();
    store.applyMoveIntoMore("reports");
    await flushPromises();
    expect(linkCount(wrapper)).toBe(3);
    store.applyMoveBackToSidebar("reports");
    await flushPromises();
    expect(linkCount(wrapper)).toBe(2);
    store.applyMoveBackToSidebar("scheduled");
    await flushPromises();
    expect(linkCount(wrapper)).toBe(2);
  });
});
