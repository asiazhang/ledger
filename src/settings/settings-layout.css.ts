import { globalStyle } from "@vanilla-extract/css";

export const SETTINGS_COLUMN_CLASS = "settings-column";

export const SETTINGS_CARD_STACK_CLASS = "settings-card-stack";

globalStyle(`.${SETTINGS_COLUMN_CLASS}`, {
  width: "100%",
});

globalStyle(`.${SETTINGS_CARD_STACK_CLASS}`, {
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr)",
  gap: "16px",
  width: "100%",
});
