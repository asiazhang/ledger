import { globalStyle } from "@vanilla-extract/css";
import { appVars } from "@ledger/theme/app-theme.css.ts";

/**
 * 出资分解只读详情（FundingDetail）旁路样式（issue #1861 / ADR-0138 决策 8，
 * ADR-0093 样式方案）：与组件同目录共置，teleport 弹窗内渲染、类名为稳定钩子类，
 * 不覆盖任何 naive 内部类。中性视觉取值一律经主题合同变量（appVars），亮暗随根
 * 主题类换装；派生标注蓝与列表侧徽标（transaction-columns 渲染单源）及同步进度
 * 条（sync-progress-bar.css.ts 先例）同一取值——info 语义色合同变量不覆盖，按
 * 先例以一次性字面量收在本文件、不入 sprinkles（不造第二套常量来源）。
 */

/** 出资分解区容器。 */
export const FUNDING_SECTION_CLASS = "funding-section";

/** 区标题（弱化小字）。 */
export const FUNDING_SECTION_TITLE_CLASS = "funding-section-title";

/** 派生标注（虚线徽标）。 */
export const FUNDING_DERIVED_CLASS = "funding-derived";

/** 出资项行。 */
export const FUNDING_ROW_CLASS = "funding-row";

/** 出资账户名（弹性省略）。 */
export const FUNDING_ACCOUNT_CLASS = "funding-account";

/** 扣款标签徽标（NTag 载体，不收缩）。 */
export const FUNDING_LABEL_CLASS = "funding-label";

/** 出资金额（表格数字对齐）。 */
export const FUNDING_AMOUNT_CLASS = "funding-amount";

/** Σ 合计行。 */
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
