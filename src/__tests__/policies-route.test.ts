import { describe, it, expect } from "vitest";
import { createMemoryHistory, createRouter } from "vue-router";
import { routes } from "@/router";
import { useSidebarOrderStore } from "@/stores/sidebar-order";

function makeRouter() {
  return createRouter({ history: createMemoryHistory(), routes });
}

describe("/policies 路由守卫（issue #706 focus 透传）", () => {
  it("收纳态（出厂种子）：/policies?focus=x 重定向 assets-more 且 focus 透传", async () => {
    const router = makeRouter();
    await router.push("/policies?focus=pol-1");
    await router.isReady();
    expect(router.currentRoute.value.name).toBe("assets-more");
    expect(router.currentRoute.value.query).toEqual({ tab: "policies", focus: "pol-1" });
  });

  it("主项态（保单已移回侧栏）：独立路由渲染，query 原样保留", async () => {
    useSidebarOrderStore().applyMoveIntoMore("investments");
    useSidebarOrderStore().applyMoveBackToSidebar("policies");
    const router = makeRouter();
    await router.push("/policies?focus=pol-1");
    await router.isReady();
    expect(router.currentRoute.value.name).toBe("policies");
    expect(router.currentRoute.value.query).toEqual({ focus: "pol-1" });
  });
});
