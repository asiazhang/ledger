import { globalStyle } from "@vanilla-extract/css";
import { appVars } from "@ledger/theme/app-theme.css.ts";

export const FUNDING_SECTION_CLASS = "funding-section";

export const FUNDING_SECTION_TITLE_CLASS = "funding-section-title";

export const FUNDING_DERIVED_CLASS = "funding-derived";

export const FUNDING_ROW_CLASS = "funding-row";

export const FUNDING_ACCOUNT_CLASS = "funding-account";

export const FUNDING_LABEL_CLASS = "funding-label";

export const FUNDING_AMOUNT_CLASS = "funding-amount";

export const FUNDING_TOTAL_CLASS = "funding-total";

globalStyle(`.${FUNDING_SECTION_CLASS}`, {
  marginTop: "14px",
});

globalStyle(`.${FUNDING_SECTION_TITLE_CLASS}`, {
  fontSize: "12px",
  fontWeight: 600,
  color: appVars.text.tertiary,
  marginBottom: "6px",
});

globalStyle(`.${FUNDING_DERIVED_CLASS}`, {
  fontSize: "12px",
  color: "#2080f0",
  border: "1px dashed currentColor",
  borderRadius: "3px",
  padding: "1px 8px",
  display: "inline-block",
  marginBottom: "6px",
});

globalStyle(`.${FUNDING_ROW_CLASS}`, {
  display: "flex",
  alignItems: "center",
  gap: "8px",
  padding: "5px 0",
});

globalStyle(`.${FUNDING_ROW_CLASS} + .${FUNDING_ROW_CLASS}`, {
  borderTop: `1px dashed ${appVars.border.divider}`,
});

globalStyle(`.${FUNDING_ACCOUNT_CLASS}`, {
  flex: "1 1 auto",
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});

globalStyle(`.${FUNDING_LABEL_CLASS}`, {
  flex: "none",
});

globalStyle(`.${FUNDING_AMOUNT_CLASS}`, {
  flex: "none",
  fontVariantNumeric: "tabular-nums",
  fontWeight: 500,
});

globalStyle(`.${FUNDING_TOTAL_CLASS}`, {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  borderTop: `1px solid ${appVars.border.divider}`,
  marginTop: "6px",
  paddingTop: "8px",
  fontSize: "12.5px",
  color: appVars.text.tertiary,
});
