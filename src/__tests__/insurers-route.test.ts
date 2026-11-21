import { describe, it, expect } from "vitest";
import { createMemoryHistory, createRouter } from "vue-router";
import { routes } from "@/router";
import { useSidebarOrderStore } from "@/stores/sidebar-order";

function makeRouter() {
  return createRouter({ history: createMemoryHistory(), routes });
}

describe("/insurers 路由守卫（issue #714）", () => {
  it("收纳态（出厂种子）：/insurers 重定向 assets-more 且带保司页签", async () => {
    const router = makeRouter();
    await router.push("/insurers");
    await router.isReady();
    expect(router.currentRoute.value.name).toBe("assets-more");
    expect(router.currentRoute.value.query).toEqual({ tab: "insurers" });
  });

  it("主项态（保司已移回侧栏）：独立路由渲染，不重定向", async () => {
    useSidebarOrderStore().applyMoveIntoMore("investments");
    useSidebarOrderStore().applyMoveBackToSidebar("insurers");
    const router = makeRouter();
    await router.push("/insurers");
    await router.isReady();
    expect(router.currentRoute.value.name).toBe("insurers");
  });
});
