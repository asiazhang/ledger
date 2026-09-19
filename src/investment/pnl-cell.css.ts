import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";

/**
 * 盈亏页金额单元格形态（ADR-0129 / ADR-0093 样式方案，组件旁路样式文件与组件共置）：
 * 「已实现收益」列 = 主值（域内算好的合计）+ 副行拆出两腿。本文件只定版式与中性色，
 * 不持语义色——两腿的颜色由 renderRealizedGainCell 经 pnlSemanticColor（主值与已实现腿）
 * 与 kindSemanticColor("dividend")（分红腿，与交易列表同源）内联给值。
 */

/** 副行：小一号、三级灰、紧贴主值；跟着单元格右对齐（text-align 继承）。 */
export const subLine = style([
  sprinkles({ display: "block", color: "textTertiary" }),
  { fontSize: "11.5px", lineHeight: "1.35", marginTop: "1px" },
]);
