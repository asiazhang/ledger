import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";

export const root = style([
  sprinkles({
    display: "flex",
    flexDirection: "column",
  }),
  {
    gap: "2px",
    pointerEvents: "none",
    userSelect: "none",
  },
]);

export const text = style({
  fontSize: "12px",
  lineHeight: 1.4,
  opacity: 0.6,
  whiteSpace: "nowrap",
});
