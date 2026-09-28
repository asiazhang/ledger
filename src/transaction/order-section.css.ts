import { globalStyle } from "@vanilla-extract/css";
import { appVars } from "@ledger/theme/app-theme.css.ts";

/**
 * 订单区（OrderSection）旁路样式（issue #1862 / ADR-0138 决策 9，ADR-0093 样式
 * 方案）：与组件同目录共置，teleport 弹窗内渲染、类名为稳定钩子类，不覆盖任何
 * naive 内部类。中性视觉取值一律经主题合同变量（appVars），亮暗随根主题类换装；
 * 徽标形态与出资项标签（funding-detail.css.ts）同一取值——语义色合同变量不覆盖
 * 的取值按先例以一次性字面量收在本文件、不入 sprinkles（不造第二套常量来源）。
 */

/** 订单区容器。 */
export const ORDER_SECTION_CLASS = "order-section";

/** 区标题（弱化小字）。 */
export const ORDER_SECTION_TITLE_CLASS = "order-section-title";

/** 订单号行（单号全文展示，可长——不省略、允许换行）。 */
export const ORDER_NO_CLASS = "order-section-no";

/** 订单指标行（行数 · 合计）。 */
export const ORDER_METRIC_CLASS = "order-section-metric";

/** 指标标签（弱化）。 */
export const ORDER_METRIC_LABEL_CLASS = "order-section-metric-label";

/** 指标值（表格数字对齐）。 */
export const ORDER_METRIC_VALUE_CLASS = "order-section-metric-value";

/** 出资构成徽标行（账户名 + 金额一组一枚）。 */
export const ORDER_ACCOUNTS_CLASS = "order-section-accounts";

/** 单枚出资构成徽标（虚线描边，与派生标注同形态）。 */
export const ORDER_ACCOUNT_TAG_CLASS = "order-section-account-tag";

/** 各行明细列表。 */
export const ORDER_ITEMS_CLASS = "order-section-items";

/** 明细行（日期 · 账户 · 金额 · 备注）。 */
export const ORDER_ITEM_ROW_CLASS = "order-section-item-row";

/** 明细弱化列（日期 / 备注）。 */
export const ORDER_ITEM_MUTED_CLASS = "order-section-item-muted";

/** 明细账户名（弹性省略）。 */
export const ORDER_ITEM_ACCOUNT_CLASS = "order-section-item-account";

/** 加载失败行。 */
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
  // 错误语义色合同变量不覆盖（text 闭集只有 primary/secondary/tertiary），
  // 按先例一次性字面量收本文件（funding-detail 派生标注蓝同款）。
  color: "#d03050",
});
