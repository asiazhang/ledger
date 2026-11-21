import { globalStyle } from "@vanilla-extract/css";
import { appVars } from "@ledger/theme/app-theme.css.ts";

export const ORDER_SECTION_CLASS = "order-section";

export const ORDER_SECTION_TITLE_CLASS = "order-section-title";

export const ORDER_NO_CLASS = "order-section-no";

export const ORDER_METRIC_CLASS = "order-section-metric";

export const ORDER_METRIC_LABEL_CLASS = "order-section-metric-label";

export const ORDER_METRIC_VALUE_CLASS = "order-section-metric-value";

export const ORDER_ACCOUNTS_CLASS = "order-section-accounts";

export const ORDER_ACCOUNT_TAG_CLASS = "order-section-account-tag";

export const ORDER_ITEMS_CLASS = "order-section-items";

export const ORDER_ITEM_ROW_CLASS = "order-section-item-row";

export const ORDER_ITEM_MUTED_CLASS = "order-section-item-muted";

export const ORDER_ITEM_ACCOUNT_CLASS = "order-section-item-account";

export const ORDER_ERROR_CLASS = "order-section-error";

globalStyle(`.${ORDER_SECTION_CLASS}`, {
  marginTop: "14px",
});

globalStyle(`.${ORDER_SECTION_TITLE_CLASS}`, {
  fontSize: "12px",
  fontWeight: 600,
  color: appVars.text.tertiary,
  marginBottom: "6px",
});

globalStyle(`.${ORDER_NO_CLASS}`, {
  fontSize: "12.5px",
  marginBottom: "6px",
  wordBreak: "break-all",
});

globalStyle(`.${ORDER_METRIC_CLASS}`, {
  display: "flex",
  alignItems: "center",
  gap: "14px",
  marginBottom: "6px",
  fontSize: "12.5px",
});

globalStyle(`.${ORDER_METRIC_LABEL_CLASS}`, {
  color: appVars.text.tertiary,
  marginRight: "4px",
});

globalStyle(`.${ORDER_METRIC_VALUE_CLASS}`, {
  fontVariantNumeric: "tabular-nums",
  fontWeight: 500,
});

globalStyle(`.${ORDER_ACCOUNTS_CLASS}`, {
  display: "flex",
  flexWrap: "wrap",
  gap: "6px",
  marginBottom: "8px",
});

globalStyle(`.${ORDER_ACCOUNT_TAG_CLASS}`, {
  fontSize: "12px",
  padding: "1px 8px",
  border: "1px dashed currentColor",
  borderRadius: "3px",
  display: "inline-flex",
  alignItems: "center",
  gap: "6px",
  maxWidth: "100%",
});

globalStyle(`.${ORDER_ITEMS_CLASS}`, {
  borderTop: `1px solid ${appVars.border.divider}`,
});

globalStyle(`.${ORDER_ITEM_ROW_CLASS}`, {
  display: "flex",
  alignItems: "center",
  gap: "8px",
  padding: "5px 0",
});

globalStyle(`.${ORDER_ITEM_ROW_CLASS} + .${ORDER_ITEM_ROW_CLASS}`, {
  borderTop: `1px dashed ${appVars.border.divider}`,
});

globalStyle(`.${ORDER_ITEM_ACCOUNT_CLASS}`, {
  flex: "1 1 auto",
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});

globalStyle(`.${ORDER_ITEM_MUTED_CLASS}`, {
  fontSize: "12px",
  color: appVars.text.tertiary,
  flex: "none",
});

globalStyle(`.${ORDER_ERROR_CLASS}`, {
  fontSize: "12px",
  color: "#d03050",
});
