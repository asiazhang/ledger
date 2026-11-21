import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";

export const banner = style([
  sprinkles({
    display: "flex",
    alignItems: "center",
  }),
  {
    position: "fixed",
    top: "env(safe-area-inset-top, 0px)",
    left: 0,
    right: 0,
    zIndex: 3000,
    gap: "8px",
    padding: "4px 12px",
    fontSize: "12px",
    lineHeight: 1.4,
    background: "var(--error-color)",
    color: "#fff",
  },
]);

export const text = style({
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  userSelect: "none",
});

export const closeButton = style({
  flexShrink: 0,
});
