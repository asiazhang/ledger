import { globalStyle } from "@vanilla-extract/css";
import { appVars } from "@ledger/theme/app-theme.css.ts";

export const PURCHASE_SECTION_CLASS = "purchase-section";

export const PURCHASE_SECTION_TITLE_CLASS = "purchase-section-title";

export const PURCHASE_ROW_CLASS = "purchase-row";

globalStyle(`.${PURCHASE_SECTION_CLASS}`, {
  marginTop: "14px",
});

globalStyle(`.${PURCHASE_SECTION_TITLE_CLASS}`, {
  fontSize: "12px",
  fontWeight: 600,
  color: appVars.text.tertiary,
  marginBottom: "6px",
});

globalStyle(`.${PURCHASE_ROW_CLASS}`, {
  padding: "5px 0",
});

globalStyle(`.${PURCHASE_ROW_CLASS} + .${PURCHASE_ROW_CLASS}`, {
  borderTop: `1px dashed ${appVars.border.divider}`,
});
