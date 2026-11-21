import { globalStyle } from "@vanilla-extract/css";

export const SYNC_HINT_CLASS = "sync-hint";

globalStyle(`.${SYNC_HINT_CLASS}`, {
  fontSize: "12px",
});
