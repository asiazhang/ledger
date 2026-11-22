import { lightTheme } from "naive-ui";
import type { Theme } from "./theme";

export interface NeutralTokens {
  radius: { small: string; base: string; large: string };
  background: { body: string; card: string; popover: string; modal: string };
  border: { border: string; divider: string };
  text: { primary: string; secondary: string; tertiary: string };
}

export const NEUTRAL_TOKENS: Record<Theme, NeutralTokens> = {
  dark: {
    radius: { small: "6px", base: "8px", large: "12px" },
    background: {
      body: "#0E0E10",
      card: "#161618",
      popover: "#1C1C1E",
      modal: "#1C1C1E",
    },
    border: {
      border: "rgba(255, 255, 255, 0.08)",
      divider: "rgba(255, 255, 255, 0.06)",
    },
    text: { primary: "#ECECEC", secondary: "#A0A0A0", tertiary: "#8A8A8A" },
  },
  light: {
    radius: {
      small: lightTheme.common.borderRadiusSmall,
      base: lightTheme.common.borderRadius,
      large: lightTheme.common.borderRadius,
    },
    background: {
      body: lightTheme.common.bodyColor,
      card: lightTheme.common.cardColor,
      popover: lightTheme.common.popoverColor,
      modal: lightTheme.common.modalColor,
    },
    border: {
      border: lightTheme.common.borderColor,
      divider: lightTheme.common.dividerColor,
    },
    text: {
      primary: lightTheme.common.textColor1,
      secondary: lightTheme.common.textColor2,
      tertiary: lightTheme.common.textColor3,
    },
  },
};

export interface FloatLayerTokens {
  color: string;
  activeOverlay: string;
  activeHoverOverlay: string;
  ringShadow: string;
}

export const DARK_FLOAT_LAYER: FloatLayerTokens = {
  color: "#343438",
  activeOverlay: "rgba(255, 255, 255, 0.06)",
  activeHoverOverlay: "rgba(255, 255, 255, 0.08)",
  ringShadow: "0 0 0 1px rgba(255, 255, 255, 0.12), 0 8px 24px rgba(0, 0, 0, 0.5)",
};
