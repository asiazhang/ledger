import { darkTheme, lightTheme } from "naive-ui";
import { darkOverrides, lightOverrides } from "./overrides";
import { NEUTRAL_TOKENS } from "./design-tokens";
import type { Theme } from "./theme";

export type TagColorType = "success" | "warning" | "info" | "default";

export interface BorderedTagColors {
  text: string;
  background: string;
  border: string;
  radius: string;
}

export interface NeutralTagColors {
  text: string;
  background: string;
  radius: string;
}

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
  return color;
}

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

export function neutralTagColors(theme: Theme): NeutralTagColors {
  const common = mergedCommon(theme);
  return {
    text: common.textColor2,
    background: common.tagColor,
    radius: NEUTRAL_TOKENS[theme].radius.small,
  };
}
