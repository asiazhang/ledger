import type { GlobalThemeOverrides } from "naive-ui";
import type { Theme } from "./theme";
import { DARK_FLOAT_LAYER, NEUTRAL_TOKENS } from "./design-tokens";

const token = NEUTRAL_TOKENS.dark;
const floatLayer = DARK_FLOAT_LAYER;

export const darkOverrides = {
  common: {
    primaryColor: "#F59E0B",
    primaryColorHover: "#FBBF24",
    primaryColorPressed: "#D97706",
    primaryColorSuppl: "#F59E0B",
    borderRadius: token.radius.base,
    borderRadiusSmall: token.radius.small,
    bodyColor: token.background.body,
    cardColor: token.background.card,
    popoverColor: token.background.popover,
    modalColor: token.background.modal,
    borderColor: token.border.border,
    dividerColor: token.border.divider,
    textColor1: token.text.primary,
    textColor2: token.text.secondary,
    textColor3: token.text.tertiary,
  },
  Dropdown: {
    color: floatLayer.color,
    peers: {
      Popover: {
        boxShadow: floatLayer.ringShadow,
      },
    },
  },
  Card: {
    borderRadius: token.radius.large,
  },
  Input: {
    borderRadius: token.radius.base,
  },
  Menu: {
    borderRadius: token.radius.small,
    itemColorActive: floatLayer.activeOverlay,
    itemColorActiveHover: floatLayer.activeHoverOverlay,
    itemTextColorActive: token.text.primary,
    itemTextColorActiveHover: token.text.primary,
    itemIconColorActive: token.text.primary,
    itemIconColorActiveHover: token.text.primary,
  },
} satisfies GlobalThemeOverrides;

export const lightOverrides = {
  common: {
    primaryColor: "#B45309",
    primaryColorHover: "#92400E",
    primaryColorPressed: "#78350F",
    primaryColorSuppl: "#B45309",
  },
} satisfies GlobalThemeOverrides;

export interface AccentColor {
  base: string;
  hover: string;
}

export function accentColor(theme: Theme): AccentColor {
  const common = theme === "dark" ? darkOverrides.common : lightOverrides.common;
  return { base: common.primaryColor, hover: common.primaryColorHover };
}
