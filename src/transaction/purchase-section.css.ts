import { globalStyle } from "@vanilla-extract/css";
import { appVars } from "@ledger/theme/app-theme.css.ts";

/**
 * 购买项清单区（PurchaseSection）旁路样式（issue #1884 / ADR-0138 决策 15，
 * ADR-0093 样式方案）：与组件同目录共置，teleport 弹窗内渲染、类名为稳定钩子类，
 * 不覆盖任何 naive 内部类。区标题、行距与分隔线取值与出资分解区
 * （funding-detail.css.ts）同型——与出资项列表同层对称的呈现节奏；中性视觉取值
 * 一律经主题合同变量（appVars），亮暗随根主题类换装。
 */

/** 购买项清单区容器。 */
export const PURCHASE_SECTION_CLASS = "purchase-section";

/** 区标题（弱化小字）。 */
export const PURCHASE_SECTION_TITLE_CLASS = "purchase-section-title";

/** 购买项行（商品名全文 + 分类/件数弱化标注，行内布局由渲染单点承载）。 */
export const PURCHASE_ROW_CLASS = "purchase-row";

globalStyle(`.${PURCHASE_SECTION_CLASS}`, {
  marginTop: "14px",
});

globalStyle(`.${PURCHASE_SECTION_TITLE_CLASS}`, {
  fontSize: "12px",
  fontWeight: 600,
  color: appVars.text.tertiary,
  marginBottom: "6px",
});

globalStyle(`.${PURCHASE_ROW_CLASS}`, {
  padding: "5px 0",
});

globalStyle(`.${PURCHASE_ROW_CLASS} + .${PURCHASE_ROW_CLASS}`, {
  borderTop: `1px dashed ${appVars.border.divider}`,
});
