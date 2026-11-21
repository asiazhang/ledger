import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";

export const root = style([
  sprinkles({
    display: "flex",
    alignItems: "center",
  }),
  {
    gap: "8px",
    minWidth: 0,
    pointerEvents: "none",
  },
]);

export const track = style({
  flex: 1,
  height: "2px",
  borderRadius: "999px",
  background: "rgba(127, 127, 127, 0.2)",
  overflow: "hidden",
});

export const bar = style({
  height: "100%",
  borderRadius: "999px",
  background: "#2080f0",
  transition: "width 0.2s ease",
  "@media": {
    "(prefers-reduced-motion: reduce)": {
      transition: "none",
    },
  },
});

export const text = style({
  fontSize: "12px",
  lineHeight: 1.4,
  opacity: 0.75,
  whiteSpace: "nowrap",
});

export const textStack = style([
  sprinkles({
    display: "flex",
    flexDirection: "column",
    alignItems: "flex-end",
  }),
  {
    gap: "2px",
    minWidth: 0,
  },
]);
