import { describe, expect, it } from "vitest";
import {
  substituteWindowTierBreakpoint,
  WINDOW_TIER_BREAKPOINT_PX as BUILD_WINDOW_TIER_BREAKPOINT_PX,
  WINDOW_TIER_CSS_TOKEN,
} from "../../vite.config";
import { WINDOW_TIER_BREAKPOINT_PX } from "@ledger/window-tier";

describe("断点唯一收口：构建期共享（CSS 占位符替换，issue #841）", () => {
  it("构建期消费的断点值与唯一收口点常量同源（vite.config 从源码提取）", () => {
    expect(BUILD_WINDOW_TIER_BREAKPOINT_PX).toBe(WINDOW_TIER_BREAKPOINT_PX);
  });

  it("CSS 占位符被替换为唯一断点常量值，占位符不再残留", () => {
    const css = `@media (max-width: ${WINDOW_TIER_CSS_TOKEN}px) { .x { color: red } }`;
    const out = substituteWindowTierBreakpoint(css);
    expect(out).not.toContain(WINDOW_TIER_CSS_TOKEN);
    expect(out).toContain(`(max-width: ${WINDOW_TIER_BREAKPOINT_PX}px)`);
    expect(out).toContain(".x { color: red }");
  });

  it("不含占位符的源码原样返回（插件零干扰前提）", () => {
    const css = "@media (prefers-reduced-motion: reduce) { .busy { opacity: 0.5 } }";
    expect(substituteWindowTierBreakpoint(css)).toBe(css);
  });
});
