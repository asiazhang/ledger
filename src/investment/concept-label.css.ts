import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";

export const conceptLabel = style([
  sprinkles({ display: "inline-flex", alignItems: "center" }),
  { gap: "4px" },
]);
