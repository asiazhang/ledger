import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";

export const subLine = style([
  sprinkles({ display: "block", color: "textTertiary" }),
  { fontSize: "11.5px", lineHeight: "1.35", marginTop: "1px" },
]);
