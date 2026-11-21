import { darkTheme, lightTheme } from "naive-ui";
import { darkOverrides, lightOverrides } from "./overrides";
import { NEUTRAL_TOKENS } from "./design-tokens";
import type { Theme } from "./theme";

/**
 * 组件库标签色解析（issue #2189 交易表格行内高频组件瘦身）：桌面交易表格的
 * 类型列与来源状态标注改用轻量语义样式元素呈现，不再逐行实例化 NTag——
 * 本模块是「NTag 四类色型在当前主题下的等价取值」的单一解析出口，公式与
 * naive-ui Tag 自身主题（es/tag/styles/{light,dark}.mjs self）逐条对齐，
 * common 取值 = naive 出厂主题 common + 本模块 overrides.common 合并
 * （与 NConfigProvider 的实际合并次序一致：overrides 覆盖出厂值）。
 *
 * 消费方（交易列工厂 transaction-columns）以当前主题解析一次、整列传入；
 * 主题切换经视图 computed 重建列即整体换色，与 NTag 响应式行为等价。
 *
 * 对齐边界：若未来给 overrides 增加 Tag 组件级覆盖或调整 Tag 相关 common 键
 * （successColor / warningColor / infoColor / textColor2 / borderColor /
 * tagColor），须同步本模块公式——组件级 Tag 覆盖出现时改走 naive 合并主题
 * 而非本快照公式（届时另票）。
 */

/** 标签色型闭集（NTag type 取值子集 = KIND_TAG_TYPE 值闭集）。 */
export type TagColorType = "success" | "warning" | "info" | "default";

/** 带 border 标签（NTag 默认 bordered）的呈现取值。 */
export interface BorderedTagColors {
  /** 文字色 */
  text: string;
  /** 背景色（暗色 = 透明，naive Tag 暗色 bordered 形态） */
  background: string;
  /** 边框（完整 border 声明） */
  border: string;
  /** 圆角（Tag 取 common.borderRadiusSmall：暗色 = token 6px，亮色 = 出厂 2px） */
  radius: string;
}

/** 无 border 标签（NTag bordered=false，来源状态标注形态）的呈现取值。 */
export interface NeutralTagColors {
  text: string;
  background: string;
  /** 圆角（同 bordered 形态：Tag 取 common.borderRadiusSmall） */
  radius: string;
}

/** 透明度叠加：hex 与 rgb()/rgba() 双形态，alpha 乘法语义同 seemly changeColor。 */
function withAlpha(color: string, alpha: number): string {
  const value = color.trim();
  if (value.startsWith("#")) {
    const hex = value.slice(1);
    const full = (
      hex.length === 3
        ? hex
            .split("")
            .map((ch) => ch + ch)
            .join("")
        : hex
    ).slice(0, 6);
    const r = parseInt(full.slice(0, 2), 16);
    const g = parseInt(full.slice(2, 4), 16);
    const b = parseInt(full.slice(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }
  const match = value.match(/^rgba?\(([^)]+)\)$/i);
  if (match) {
    const parts = match[1]
      .split(/[\s,/]+/)
      .filter(Boolean)
      .map(Number);
    const [r, g, b, a = 1] = parts;
    return `rgba(${r}, ${g}, ${b}, ${Number((a * alpha).toFixed(4))})`;
  }
  // 不可解析（防御分支）：原样返回，透明度叠加缺失但不致渲染中断
  return color;
}

/** 当前主题的合并 common（naive 出厂 + overrides.common，次序同 NConfigProvider）。 */
function mergedCommon(theme: Theme): Record<string, string> {
  const naive = theme === "dark" ? darkTheme.common : lightTheme.common;
  const overrides = (theme === "dark" ? darkOverrides.common : lightOverrides.common) as Record<
    string,
    string | undefined
  >;
  const merged: Record<string, string> = {};
  for (const [key, value] of Object.entries(naive)) {
    if (typeof value === "string") merged[key] = value;
  }
  for (const [key, value] of Object.entries(overrides)) {
    if (typeof value === "string") merged[key] = value;
  }
  return merged;
}

/**
 * 带边框标签四类色型（NTag 默认形态的轻量等价取值）：
 * - 三语义色型（success/warning/info）：文字 = 语义色；亮色背景 = 语义色 α0.1
 *   （naive colorBordered 公式），暗色背景 = 透明（naive 暗色 colorBordered = #0000）；
 *   边框 = 语义色 α0.3（naive border 公式，两主题同式）。
 * - default：文字 = textColor2、边框 = borderColor（合并 common），亮色背景 =
 *   naive colorBordered 出厂值 rgb(250,250,252)、暗色透明。
 */
export function borderedTagColors(theme: Theme): Record<TagColorType, BorderedTagColors> {
  const common = mergedCommon(theme);
  const radius = NEUTRAL_TOKENS[theme].radius.small;
  const typed = (color: string): BorderedTagColors => ({
    text: color,
    background: theme === "light" ? withAlpha(color, 0.1) : "transparent",
    border: `1px solid ${withAlpha(color, 0.3)}`,
    radius,
  });
  return {
    success: typed(common.successColor),
    warning: typed(common.warningColor),
    info: typed(common.infoColor),
    default: {
      text: common.textColor2,
      background: theme === "light" ? "rgb(250, 250, 252)" : "transparent",
      border: `1px solid ${common.borderColor}`,
      radius,
    },
  };
}

/**
 * 无边框中性标签（来源状态标注「已删除/已取消」的 NTag size=small bordered=false
 * 轻量等价取值）：文字 = textColor2、背景 = tagColor（naive Tag 无类型填充色，
 * 亮 #eee / 暗 rgb(51,51,51)，app overrides 未触碰该键）。
 */
export function neutralTagColors(theme: Theme): NeutralTagColors {
  const common = mergedCommon(theme);
  return {
    text: common.textColor2,
    background: common.tagColor,
    radius: NEUTRAL_TOKENS[theme].radius.small,
  };
}
