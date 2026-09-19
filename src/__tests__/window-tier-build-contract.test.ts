import { describe, expect, it } from "vitest";
import {
  substituteWindowTierBreakpoint,
  WINDOW_TIER_BREAKPOINT_PX as BUILD_WINDOW_TIER_BREAKPOINT_PX,
  WINDOW_TIER_CSS_TOKEN,
} from "../../vite.config";
import { WINDOW_TIER_BREAKPOINT_PX } from "@ledger/window-tier";

/**
 * 断点唯一收口的构建期契约测试（issue #841，ADR-0088 决策 2；issue #1315 起留壳侧）：
 * 被测对象是 vite.config.ts 的构建期提取与占位符替换（壳侧构建配置，非包内代码），
 * 且断言需相对导入 vite.config——包内禁止相对路径穿越（check-frontend-structure.ts
 * 规则③），故与包内 composable 测试（packages/window-tier/src/__tests__）分置，
 * 断言原文未动。
 */

describe("断点唯一收口：构建期共享（CSS 占位符替换，issue #841）", () => {
  it("构建期消费的断点值与唯一收口点常量同源（vite.config 从源码提取）", () => {
    expect(BUILD_WINDOW_TIER_BREAKPOINT_PX).toBe(WINDOW_TIER_BREAKPOINT_PX);
  });

  it("CSS 占位符被替换为唯一断点常量值，占位符不再残留", () => {
    const css = `@media (max-width: ${WINDOW_TIER_CSS_TOKEN}px) { .x { color: red } }`;
    const out = substituteWindowTierBreakpoint(css);
    expect(out).not.toContain(WINDOW_TIER_CSS_TOKEN);
    expect(out).toContain(`(max-width: ${WINDOW_TIER_BREAKPOINT_PX}px)`);
    expect(out).toContain(".x { color: red }"); // 非占位符内容原样保留
  });

  it("不含占位符的源码原样返回（插件零干扰前提）", () => {
    const css = "@media (prefers-reduced-motion: reduce) { .busy { opacity: 0.5 } }";
    expect(substituteWindowTierBreakpoint(css)).toBe(css);
  });
});
