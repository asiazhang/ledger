import { globalStyle } from "@vanilla-extract/css";

export const INSTRUMENT_LINK_CLASS = "instrument-link";

globalStyle(`.${INSTRUMENT_LINK_CLASS}`, {
  border: "none",
  padding: 0,
  background: "none",
  font: "inherit",
  cursor: "pointer",
  borderRadius: "4px",
  maxWidth: "100%",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});

globalStyle(`.${INSTRUMENT_LINK_CLASS}:hover`, {
  color: "var(--accent-hover)",
  background: "rgba(255, 255, 255, 0.06)",
  textDecoration: "underline",
});

globalStyle(`.${INSTRUMENT_LINK_CLASS}:focus-visible`, {
  outline: "2px solid var(--accent-hover)",
  outlineOffset: "2px",
});

export const INSTRUMENT_PLACEHOLDER_CLASS = "instrument-placeholder";

globalStyle(`.${INSTRUMENT_PLACEHOLDER_CLASS}`, {
  color: "inherit",
  maxWidth: "100%",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
