import { globalStyle } from "@vanilla-extract/css";

export const MOBILE_CARD_CLASS = "app-modal-mobile-card";

globalStyle(`.${MOBILE_CARD_CLASS}`, {
  overflow: "hidden",
});

globalStyle(`.${MOBILE_CARD_CLASS} .n-card-content`, {
  display: "flex",
  flexDirection: "column",
  minHeight: 0,
  overflowY: "auto",
});

globalStyle(`.${MOBILE_CARD_CLASS} .n-card-content > *`, {
  flexShrink: 0,
});

globalStyle(`.${MOBILE_CARD_CLASS} .n-form-item.n-form-item--left-labelled`, {
  gridTemplateAreas: '"label" "blank" "feedback"',
  gridTemplateColumns: "minmax(0, 100%)",
  gridTemplateRows: "minmax(var(--n-label-height), auto) 1fr",
  alignItems: "stretch",
});

globalStyle(`.${MOBILE_CARD_CLASS} .n-form-item--left-labelled .n-form-item-label`, {
  textAlign: "left",
});

globalStyle(
  `.${MOBILE_CARD_CLASS} .n-descriptions--left-label-placement .n-descriptions-table-header`,
  { whiteSpace: "nowrap" },
);

globalStyle(`.${MOBILE_CARD_CLASS} form.n-form:not(.n-form--inline)`, {
  display: "flex",
  flexDirection: "column",
  flexGrow: 1,
});

globalStyle(
  `.${MOBILE_CARD_CLASS} form.n-form:not(.n-form--inline) > .n-space, .${MOBILE_CARD_CLASS} .n-card-content > .n-space`,
  {
    flexGrow: 1,
  },
);

globalStyle(
  `.${MOBILE_CARD_CLASS} form.n-form:not(.n-form--inline) > .n-space > :last-child, .${MOBILE_CARD_CLASS} .n-card-content > .n-space > :last-child`,
  {
    marginTop: "auto",
  },
);
