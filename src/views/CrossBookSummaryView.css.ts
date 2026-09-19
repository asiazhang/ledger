import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";

/**
 * 跨账本投资汇总页旁路样式（issue #1196 / ADR-0093 样式方案）：与组件同目录
 * 共置。合计卡为自适应栅格，逐本状态行沿用账本清单行的宽松行距语言；无中性
 * token 的轴（字号/行高）以一次性字面量收在本文件，不入 sprinkles（不造第二套
 * 常量来源）。
 */
export const summaryRoot = style([
  sprinkles({
    display: "flex",
    flexDirection: "column",
  }),
  {
    gap: "12px",
  },
]);

export const cardsGrid = style({
  display: "grid",
  gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))",
  gap: "12px",
});

export const cardLabel = style({
  fontSize: "12px",
});

export const cardAmount = style({
  marginTop: "4px",
  fontSize: "20px",
  fontWeight: 600,
});

export const bookRow = style([
  sprinkles({
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
  }),
  {
    gap: "8px",
    padding: "6px 0",
  },
]);

export const bookName = style([
  sprinkles({
    display: "flex",
    alignItems: "center",
  }),
  {
    gap: "6px",
    minWidth: 0,
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  },
]);

export const statusIcon = style({
  verticalAlign: "-1px",
});
