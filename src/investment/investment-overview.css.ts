import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";
import { appVars } from "@ledger/theme/app-theme.css.ts";

export const hero = style([
  sprinkles({ display: "flex", flexDirection: "column" }),
  { gap: "4px" },
]);

export const heroHeader = style([
  sprinkles({ display: "flex", alignItems: "center", justifyContent: "space-between" }),
  { gap: "12px" },
]);

export const heroValue = style([
  sprinkles({ color: "textPrimary" }),
  { fontSize: "48px", lineHeight: "1.15", fontWeight: 600 },
]);

export const compositionBar = style([
  sprinkles({ display: "flex", borderRadius: "small" }),
  {
    height: "10px",
    overflow: "hidden",
    background: appVars.border.divider,
  },
]);

const CASH_COLOR = appVars.text.tertiary;

const HOLDINGS_COLOR = appVars.text.primary;

const barSegmentBase = style({ height: "100%" });

export const barCash = style([barSegmentBase, { background: CASH_COLOR }]);

export const barHoldings = style([barSegmentBase, { background: HOLDINGS_COLOR }]);

export const legend = style([
  sprinkles({ display: "flex", alignItems: "center" }),
  { gap: "16px", flexWrap: "wrap" },
]);

export const legendItem = style([
  sprinkles({ display: "inline-flex", alignItems: "center" }),
  { gap: "6px" },
]);

const legendDotBase = style({
  flex: "none",
  width: "8px",
  height: "8px",
  borderRadius: appVars.radius.small,
});

export const legendDotCash = style([legendDotBase, { background: CASH_COLOR }]);

export const legendDotHoldings = style([legendDotBase, { background: HOLDINGS_COLOR }]);
