import { describe, expect, it } from "vitest";
import { deriveViewShortcuts } from "@/composables/useViewShortcuts";
import { useSidebarOrderStore } from "@/stores/sidebar-order";
import { viewLabel } from "@ledger/i18n/view-label";
import zhCN from "@ledger/i18n/locales/zh-CN";

describe("viewLabel（视图标题文案）", () => {
  it("全部视图名解析为文案而非 key 代号（漏域名前缀回归）", () => {
    for (const { name } of deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders)) {
      const label = viewLabel(name);
      expect(label, name).toBeTruthy();
      expect(label, name).not.toContain("nav.");
    }
  });

  it("key 契约：文案挂在 common.nav 下且覆盖全部视图名", () => {
    for (const { name } of deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders)) {
      expect(zhCN.common.nav, name).toHaveProperty(name);
    }
  });

  it("zh-CN 解析示例：dashboard → 概览", () => {
    expect(viewLabel("dashboard")).toBe("概览");
  });

  it("组内「更多」路由名解析为文案（issue #472 / ADR-0063：内容区标题同源，新增键 zh/en 同步）", () => {
    for (const name of ["bookkeeping-more", "assets-more", "insights-more"]) {
      expect(zhCN.common.nav, name).toHaveProperty(name);
      const label = viewLabel(name);
      expect(label, name).toBeTruthy();
      expect(label, name).not.toContain("nav.");
    }
    expect(viewLabel("assets-more")).toBe("资产 · 更多");
  });
});
