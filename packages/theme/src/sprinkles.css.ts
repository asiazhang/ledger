import { createSprinkles, defineProperties } from "@vanilla-extract/sprinkles";
import { appVars } from "./app-theme.css.ts";

const properties = defineProperties({
  properties: {
    display: ["block", "inline-block", "flex", "inline-flex", "none"],
    flexDirection: ["row", "column"],
    alignItems: ["stretch", "flex-start", "center", "flex-end", "baseline"],
    justifyContent: ["flex-start", "center", "flex-end", "space-between", "space-around"],
    borderRadius: {
      small: appVars.radius.small,
      base: appVars.radius.base,
      large: appVars.radius.large,
    },
    background: {
      body: appVars.background.body,
      card: appVars.background.card,
      popover: appVars.background.popover,
      modal: appVars.background.modal,
    },
    borderColor: {
      border: appVars.border.border,
      divider: appVars.border.divider,
    },
    color: {
      textPrimary: appVars.text.primary,
      textSecondary: appVars.text.secondary,
      textTertiary: appVars.text.tertiary,
    },
  },
});

export const sprinkles = createSprinkles(properties);
