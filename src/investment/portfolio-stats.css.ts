import { style } from "@vanilla-extract/css";
import { sprinkles } from "@ledger/theme/sprinkles.css.ts";
import { appVars } from "@ledger/theme/app-theme.css.ts";

/**
 * 投资合计三卡（总市值 / 持仓收益 / 累计收益）的形态样式（ADR-0093 样式方案，
 * 组件旁路样式文件与组件共置）：每格独立成一张细边框卡片、三张同排（等宽与窄窗
 * 单列归 NGrid），与下方内容分层——细边框 + 微凹底面（暗色下比外层卡片低一阶，
 * 亮色下由边框承担分界）。issue #1797 起三卡改折本位币单值，多币种分组连接符样式
 * 随展示形态一并退役。
 *
 * 颜色只给中性兜底：数值默认取主文本色——组件库 NStatistic 出厂取次级色偏灰，
 * 与「净资产」卡的主数字（NText strong）不一致。盈亏两卡的语义色不在此定义，
 * 由 semantic-colors 接缝逐币种内联给值（词汇表「盈亏涨跌色」——涨跌色不外溢
 * 到市值，故本文件不持任何业务语义色）。
 */
export const statsCard = style([
  sprinkles({ borderRadius: "base", display: "flex", flexDirection: "column" }),
  {
    // 三卡同排等宽等高：卡片吃满网格行高，多币种换行的卡不把同排其余卡拉矮
    height: "100%",
    boxSizing: "border-box",
    // 分层靠亮度台阶、不靠描边（ADR-0093 暗色浮层语言同向）：凹面带改为比外层卡片
    // 抬亮一阶，去掉外层卡片里再套一组描边盒的“盒中盒”观感。
    background: appVars.background.popover,
    padding: "12px 16px",
  },
]);

/** 卡头标签行：概念名与口径说明触发器同排居中（触发器不换行、不撑高标签行） */
export const statsLabel = style([
  sprinkles({ display: "inline-flex", alignItems: "center" }),
  { gap: "4px" },
]);

/** 数值的中性兑底色；等宽数字由 NStatistic 的 tabular-nums 属性承担 */
export const statsValue = style({
  color: appVars.text.primary,
});
