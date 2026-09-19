import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";

/**
 * 口径说明标签形态（ADR-0093 样式方案，组件旁路样式文件与组件共置）：
 * 标签与常驻 `ⓘ` 同排、间距 4px，触发器不换行、不撑高所在行（表格列头与卡片
 * 标签行共用同一形态）。颜色沿用组件库标签色（组件内联取 --n-label-text-color），
 * 本文件只定版式、不持语义色。
 */
export const conceptLabel = style([
  sprinkles({ display: "inline-flex", alignItems: "center" }),
  { gap: "4px" },
]);
