import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";

export const summaryRoot = style([
  sprinkles({
    display: "flex",
    flexDirection: "column",
  }),
  {
    gap: "12px",
  },
]);

export const cardsGrid = style({
  display: "grid",
  gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))",
  gap: "12px",
});

export const cardLabel = style({
  fontSize: "12px",
});

export const cardAmount = style({
  marginTop: "4px",
  fontSize: "20px",
  fontWeight: 600,
});

export const bookRow = style([
  sprinkles({
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
  }),
  {
    gap: "8px",
    padding: "6px 0",
  },
]);

export const bookName = style([
  sprinkles({
    display: "flex",
    alignItems: "center",
  }),
  {
    gap: "6px",
    minWidth: 0,
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  },
]);

export const statusIcon = style({
  verticalAlign: "-1px",
});
