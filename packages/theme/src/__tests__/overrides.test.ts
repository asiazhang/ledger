import { describe, expect, it } from "vitest";
import { accentColor, darkOverrides, lightOverrides } from "../overrides";

/** 强调色选择器（issue #1268）：组件层强调色的单一解析出口。
 * 只测外部行为：亮/暗取值定案钉住、与 overrides 值源同源（选择器不持第二份
 * 色值字面量，改 overrides 即改选择器产出）。 */

describe("accentColor（按主题取强调色）", () => {
  it("暗色：琥珀强调色（与 darkOverrides.common 定案值一致）", () => {
    expect(accentColor("dark")).toEqual({
      base: "#F59E0B",
      hover: "#FBBF24",
    });
  });

  it("亮色：同色相加深版（与 lightOverrides.common 定案值一致）", () => {
    expect(accentColor("light")).toEqual({
      base: "#B45309",
      hover: "#92400E",
    });
  });

  it("与 overrides 值源同源派生：不新增第二份色值字面量", () => {
    expect(accentColor("dark")).toEqual({
      base: darkOverrides.common.primaryColor,
      hover: darkOverrides.common.primaryColorHover,
    });
    expect(accentColor("light")).toEqual({
      base: lightOverrides.common.primaryColor,
      hover: lightOverrides.common.primaryColorHover,
    });
  });

  it("亮暗取值彼此不同（亮色确实是加深变体）", () => {
    expect(accentColor("light")).not.toEqual(accentColor("dark"));
  });
});
