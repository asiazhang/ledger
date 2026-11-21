import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";
import { appVars } from "@ledger/theme/app-theme.css.ts";

export const statsCard = style([
  sprinkles({ borderRadius: "base", display: "flex", flexDirection: "column" }),
  {
    height: "100%",
    boxSizing: "border-box",
    background: appVars.background.popover,
    padding: "12px 16px",
  },
]);

export const statsLabel = style([
  sprinkles({ display: "inline-flex", alignItems: "center" }),
  { gap: "4px" },
]);

export const statsValue = style({
  color: appVars.text.primary,
});
