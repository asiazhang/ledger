import { globalStyle } from "@vanilla-extract/css";

export const TX_CELL_LINK_CLASS = "tx-cell-link";
export const TX_CELL_PLACEHOLDER_CLASS = "tx-cell-placeholder";
export const TX_KIND_TAG_CLASS = "tx-kind-tag";
export const TX_SOURCE_CELL_CLASS = "tx-source-cell";
export const TX_SOURCE_ICON_CLASS = "tx-source-icon";
export const TX_SOURCE_TYPE_LABEL_CLASS = "tx-source-type-label";
export const TX_SOURCE_STATUS_TAG_CLASS = "tx-source-status-tag";
export const TX_NOTE_CELL_CLASS = "tx-note-cell";

globalStyle(`.tx-cell-link`, {
  border: "none",
  padding: "0",
  background: "none",
  font: "inherit",
  cursor: "pointer",
  borderRadius: "4px",
  minWidth: "0",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  textAlign: "left",
});

globalStyle(`.tx-cell-link:hover`, {
  color: "var(--tx-accent-hover)",
  background: "rgba(255, 255, 255, 0.06)",
  textDecoration: "underline",
});

globalStyle(`.tx-cell-link:focus-visible`, {
  outline: "2px solid var(--tx-accent-hover)",
  outlineOffset: "2px",
});

globalStyle(`.tx-cell-placeholder`, {
  maxWidth: "100%",
  minWidth: "0",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});

globalStyle(`.tx-kind-tag`, {
  display: "inline-flex",
  alignItems: "center",
  boxSizing: "border-box",
  height: "28px",
  padding: "0 7px",
  fontSize: "14px",
  lineHeight: "1",
  whiteSpace: "nowrap",
});

globalStyle(`.tx-source-cell`, {
  display: "inline-flex",
  alignItems: "center",
  gap: "4px",
  maxWidth: "100%",
  color: "inherit",
  opacity: "0.92",
});

globalStyle(`.tx-source-icon`, {
  flex: "none",
  width: "1em",
  height: "1em",
  opacity: "0.65",
});

globalStyle(`.tx-source-type-label`, {
  flex: "none",
  fontSize: "11px",
  opacity: "0.55",
});

globalStyle(`.tx-source-status-tag`, {
  display: "inline-flex",
  alignItems: "center",
  boxSizing: "border-box",
  flex: "none",
  height: "22px",
  padding: "0 7px",
  fontSize: "12px",
  lineHeight: "1",
  whiteSpace: "nowrap",
});

globalStyle(`.n-data-table .note-copy-btn, .n-data-table .row-actions-btn`, {
  border: "none",
  background: "none",
  padding: "0",
  cursor: "pointer",
  color: "inherit",
  font: "inherit",
  borderRadius: "4px",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
});

globalStyle(
  `.n-data-table .note-copy-btn:focus-visible, .n-data-table .row-actions-btn:focus-visible`,
  {
    outline: "2px solid var(--tx-accent-hover, currentColor)",
    outlineOffset: "1px",
  },
);

globalStyle(`.n-data-table .note-copy-btn svg`, {
  width: "14px",
  height: "14px",
});

globalStyle(`.tx-note-cell`, {
  display: "flex",
  alignItems: "center",
  gap: "2px",
  width: "100%",
  maxWidth: "100%",
});
