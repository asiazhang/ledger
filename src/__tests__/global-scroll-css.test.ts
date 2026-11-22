import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

interface CssRule {
  selectors: string[];
  declarations: Record<string, string>;
}

function parseTopLevelRules(css: string): CssRule[] {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const rules: CssRule[] = [];
  const blockRe = /([^{}@]+)\{([^{}]+)\}/g;
  for (const match of withoutComments.matchAll(blockRe)) {
    const selectors = match[1]
      .split(",")
      .map((s) => s.trim().replace(/\s+/g, " "))
      .filter(Boolean);
    const declarations: Record<string, string> = {};
    for (const part of match[2].split(";")) {
      const idx = part.indexOf(":");
      if (idx === -1) continue;
      const prop = part.slice(0, idx).trim().toLowerCase();
      const val = part
        .slice(idx + 1)
        .trim()
        .toLowerCase();
      if (prop) declarations[prop] = val;
    }
    rules.push({ selectors, declarations });
  }
  return rules;
}

describe("全局滚动样式契约", () => {
  const cssPath = resolve(process.cwd(), "src/assets/global.css");
  const rules = parseTopLevelRules(readFileSync(cssPath, "utf-8"));

  it("html、body 与外层 .n-layout-scroll-container 保留 overscroll-behavior: none 防橡皮筋白条", () => {
    for (const selector of ["html", "body", ".n-layout-scroll-container"]) {
      const hasNone = rules.some(
        (r) => r.selectors.includes(selector) && r.declarations["overscroll-behavior"] === "none",
      );
      expect(hasNone, `${selector} 应声明 overscroll-behavior: none`).toBe(true);
    }
  });

  it("嵌套的 .n-scrollbar-container 不得阻断滚动链（避免吞掉滚轮/触摸板滚动）", () => {
    const blockingRules = rules.filter((r) => {
      if (!r.selectors.some((s) => s.includes(".n-scrollbar-container"))) {
        return false;
      }
      const value =
        r.declarations["overscroll-behavior"] ?? r.declarations["overscroll-behavior-y"] ?? "";
      return value.includes("none") || value.includes("contain");
    });
    expect(blockingRules).toEqual([]);
  });

  it("桌面壳层包含定高列表视图时锁紧视口高度且为列表区保留 240px 极矮窗口兜底", () => {
    const shellLocked = rules.some(
      (r) =>
        r.selectors.includes(".app-page-shell:has(.viewport-table-view)") &&
        r.declarations["height"] === "100%",
    );
    expect(shellLocked).toBe(true);

    for (const selector of [
      ".app-page-shell .viewport-table-slot",
      ".app-page-shell .viewport-table-card",
    ]) {
      const hasFloor = rules.some(
        (r) => r.selectors.includes(selector) && r.declarations["min-height"] === "240px",
      );
      expect(hasFloor, `${selector} 应保留 min-height: 240px 兜底`).toBe(true);
    }
  });
});
