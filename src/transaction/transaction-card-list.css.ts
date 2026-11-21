import { globalStyle } from "@vanilla-extract/css";
import { appVars } from "@ledger/theme/app-theme.css.ts";

export const TRANSACTION_CARD_LIST_CLASS = "transaction-card-list";

export const TRANSACTION_CARD_CLASS = "transaction-card";

export const CARD_MENU_CLASS = "transaction-card-menu";

globalStyle(`.${TRANSACTION_CARD_LIST_CLASS}`, {
  display: "flex",
  flexDirection: "column",
  gap: "8px",
});

globalStyle(`.${TRANSACTION_CARD_CLASS}`, {
  display: "flex",
  flexDirection: "column",
  gap: "6px",
  padding: "12px",
  borderRadius: appVars.radius.base,
  background: appVars.background.card,
  border: `1px solid ${appVars.border.border}`,
});

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-head`, {
  display: "flex",
  alignItems: "center",
  gap: "8px",
});

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-date`, {
  fontSize: "12px",
  color: appVars.text.secondary,
  fontVariantNumeric: "tabular-nums",
  flex: "none",
});

globalStyle(`.${TRANSACTION_CARD_CLASS} .${CARD_MENU_CLASS}`, {
  marginLeft: "auto",
});

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-row`, {
  display: "flex",
  alignItems: "center",
  gap: "6px",
  minWidth: 0,
  fontSize: "13px",
  whiteSpace: "nowrap",
  overflow: "hidden",
});

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-purchase`, {
  whiteSpace: "normal",
});

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-row > *`, {
  flexShrink: 1,
  minWidth: 0,
});

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-empty`, {
  color: appVars.text.tertiary,
});

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-join`, {
  color: appVars.text.tertiary,
  flex: "none",
});

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-amount`, {
  display: "flex",
  justifyContent: "flex-end",
  fontSize: "16px",
  fontWeight: 600,
  fontVariantNumeric: "tabular-nums",
  paddingTop: "2px",
});
