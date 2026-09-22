import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";
import { appVars } from "@ledger/theme/app-theme.css.ts";

/**
 * 投资概览面板「焦点 Hero」版式（spec #1684，ADR-0093 样式方案，组件旁路样式
 * 文件与组件共置）：首屏焦点（可投资资产大号数字 + 右上本位币标注）→ 两段式
 * 构成比例条 + 图例 → 投资合计一行细条。
 *
 * 颜色只取中性令牌：比例条两段与图例色点用文字灰阶两阶（现金 = 三级灰、持仓 =
 * 主文本色），轨道取分隔线色——构成比例是展示派生的纯图形，不引入任何业务语义
 * 色，总市值与可投资资产保持中性；盈亏着色经 semantic-colors 接缝由组件内联给值，
 * 本文件不持任何语义色。
 */

/** 首屏焦点区（spec #1684）：标签行 + 大号数字纵排，占卡片首行 */
export const hero = style([
  sprinkles({ display: "flex", flexDirection: "column" }),
  { gap: "4px" },
]);

/** 焦点头行：概念标签（含常驻 ⓘ）居左、本位币标注退到右上角 */
export const heroHeader = style([
  sprinkles({ display: "flex", alignItems: "center", justifyContent: "space-between" }),
  { gap: "12px" },
]);

/**
 * 焦点大号数字（原型值 48px，可随栅格微调）：中性主文本色（可投资资产保持
 * 中性，涨跌色不外溢）；等宽数字由全局工具类 `tabular-nums` 单点承担。
 */
export const heroValue = style([
  sprinkles({ color: "textPrimary" }),
  { fontSize: "48px", lineHeight: "1.15", fontWeight: 600 },
]);

/** 构成比例条轨道：分隔线色空轨（可投资资产为 0 与隐私模式下整条不渲染，判定在组件） */
export const compositionBar = style([
  sprinkles({ display: "flex", borderRadius: "small" }),
  {
    height: "10px",
    overflow: "hidden",
    background: appVars.border.divider,
  },
]);

/** 现金腿颜色：段与图例色点同源一处（改色只动这里，两处自动同步） */
const CASH_COLOR = appVars.text.tertiary;

/** 持仓腿颜色：同为中性灰阶另一阶，随主题整体换装 */
const HOLDINGS_COLOR = appVars.text.primary;

/** 比例段共形：段高充满轨道，宽度由组件按各腿份额内联给值 */
const barSegmentBase = style({ height: "100%" });

/** 现金段：三级灰 */
export const barCash = style([barSegmentBase, { background: CASH_COLOR }]);

/** 持仓段：主文本色 */
export const barHoldings = style([barSegmentBase, { background: HOLDINGS_COLOR }]);

/** 图例行：色点 + 既有两腿标签键 + 格式化金额；窄窗换行降列不溢出 */
export const legend = style([
  sprinkles({ display: "flex", alignItems: "center" }),
  { gap: "16px", flexWrap: "wrap" },
]);

/** 图例单项：色点与文案同排不换行（点掉了文案就断了归属） */
export const legendItem = style([
  sprinkles({ display: "inline-flex", alignItems: "center" }),
  { gap: "6px" },
]);

/** 图例色点共形：8px 圆点、装饰性（对辅助技术隐藏），与对应比例段同色 */
const legendDotBase = style({
  flex: "none",
  width: "8px",
  height: "8px",
  borderRadius: appVars.radius.small,
});

/** 图例色点（现金腿）：与现金段同源取色 */
export const legendDotCash = style([legendDotBase, { background: CASH_COLOR }]);

/** 图例色点（持仓腿）：与持仓段同源取色 */
export const legendDotHoldings = style([legendDotBase, { background: HOLDINGS_COLOR }]);
