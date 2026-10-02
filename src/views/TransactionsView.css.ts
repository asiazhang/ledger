import { style } from "@vanilla-extract/css";

/**
 * 交易页旁路样式（ADR-0093 样式方案）：与视图同目录共置。翻页快捷键提示
 * （issue #1902 / ADR-0140）为弱化小字——桌面档右对齐贴表格底部分页条，
 * 移动档分页条 suffix 同类；颜色随主题（naive textColor3）在模板侧内联绑定，
 * 字号/对齐为一次性字面量，不入 sprinkles（不造第二套常量来源）。
 */
export const paginationHint = style({
  fontSize: "12px",
  textAlign: "right",
});
