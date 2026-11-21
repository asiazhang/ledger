import { describe, expect, it } from "vitest";
import { darkTheme, lightTheme } from "naive-ui";
import { darkOverrides, lightOverrides } from "../overrides";
import { NEUTRAL_TOKENS } from "../design-tokens";
import { borderedTagColors, neutralTagColors } from "../tag-colors";

/**
 * 标签色解析契约（issue #2189 轻量类型标签/来源状态标注）：解析产物与
 * naive-ui Tag 自身公式逐条对齐（见 tag-colors.ts 头注），本测试锁定公式
 * 输入输出——naive 出厂 common 漂移或公式被改动时此处的独立推导断言变红。
 */

describe("borderedTagColors（NTag 默认 bordered 形态）", () => {
  it("暗色：语义色型文字 = 语义色、背景透明、边框 = 语义色 α0.3；default = 合并 textColor2/borderColor", () => {
    const tags = borderedTagColors("dark");
    const success = darkTheme.common.successColor;
    expect(tags.success.text).toBe(success);
    expect(tags.success.background).toBe("transparent");
    expect(tags.success.border).toBe("1px solid rgba(99, 226, 183, 0.3)");
    // overrides 合并次序：default 文字/边框取 app darkOverrides（textColor2 / borderColor）
    expect(tags.default.text).toBe(darkOverrides.common.textColor2);
    expect(tags.default.border).toBe(`1px solid ${darkOverrides.common.borderColor}`);
    expect(tags.default.background).toBe("transparent");
    // 圆角随 token（dark radius.small = 6px）
    expect(tags.success.radius).toBe(NEUTRAL_TOKENS.dark.radius.small);
  });

  it("亮色：语义色型背景 = 语义色 α0.1（colorBordered 公式）；default 背景 = naive colorBordered 出厂值", () => {
    const tags = borderedTagColors("light");
    expect(tags.warning.text).toBe(lightTheme.common.warningColor);
    expect(tags.warning.background).toBe("rgba(240, 160, 32, 0.1)");
    expect(tags.warning.border).toBe("1px solid rgba(240, 160, 32, 0.3)");
    expect(tags.default.text).toBe(lightTheme.common.textColor2);
    expect(tags.default.border).toBe(`1px solid ${lightTheme.common.borderColor}`);
    expect(tags.default.background).toBe("rgb(250, 250, 252)");
    expect(tags.info.radius).toBe(NEUTRAL_TOKENS.light.radius.small);
  });

  it("亮色 overrides 不触达标签键：语义色取 naive 出厂（app lightOverrides 只覆盖强调色）", () => {
    const tags = borderedTagColors("light");
    expect(tags.success.text).toBe(lightTheme.common.successColor);
    // 强调色覆盖不影响标签（success/warning/info 与 primary 无关）
    expect(tags.success.text).not.toBe(lightOverrides.common.primaryColor);
  });
});

describe("neutralTagColors（NTag bordered=false 填充形态）", () => {
  it("文字 = 合并 textColor2、背景 = tagColor（overrides 未触达 → naive 出厂值）", () => {
    expect(neutralTagColors("dark")).toEqual({
      text: darkOverrides.common.textColor2,
      background: darkTheme.common.tagColor,
    });
    expect(neutralTagColors("light")).toEqual({
      text: lightTheme.common.textColor2,
      background: lightTheme.common.tagColor,
    });
  });
});
