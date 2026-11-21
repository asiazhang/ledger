import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";
import { appVars } from "@ledger/theme/app-theme.css.ts";

export const promptBody = style([
  sprinkles({
    borderRadius: "small",
    background: "card",
  }),
  {
    margin: 0,
    padding: "12px 16px",
    border: `1px solid ${appVars.border.border}`,
    fontFamily: "ui-monospace, 'SF Mono', Menlo, Consolas, monospace",
    fontSize: "13px",
    lineHeight: 1.7,
    whiteSpace: "pre-wrap",
    wordBreak: "break-word",
    maxHeight: "60vh",
    overflow: "auto",
  },
]);
