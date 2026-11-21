import { globalStyle } from "@vanilla-extract/css";
import { appVars } from "@ledger/theme/app-theme.css.ts";

export const CREATE_FAB_CLASS = "create-fab";

globalStyle(`.${CREATE_FAB_CLASS}.n-button`, {
  position: "fixed",
  right: "16px",
  bottom: "calc(16px + env(safe-area-inset-bottom))",
  width: "56px",
  height: "56px",
  fontSize: "28px",
  borderRadius: "50%",
});

export const CREATE_FAB_SHEET_CLASS = "create-fab-sheet";

globalStyle(`.${CREATE_FAB_SHEET_CLASS}`, {
  display: "flex",
  flexDirection: "column",
  gap: "8px",
  minWidth: "180px",
});

export const CREATE_FAB_OPTION_CLASS = "create-fab-option";

globalStyle(`.${CREATE_FAB_OPTION_CLASS}.n-button`, {
  width: "100%",
  minHeight: "48px",
  justifyContent: "center",
  background: appVars.background.card,
});
