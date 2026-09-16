import { style } from '@vanilla-extract/css'

/**
 * 资金加权收益率单元格（MwrRateCell）的形态样式（ADR-0093 样式方案，组件旁路
 * 样式文件与组件共置）：只有角标与触发器的两处弱化形态，不持业务语义色——
 * 数值颜色由 renderMwrRateCell 经 pnlSemanticColor 内联给值（词汇表「盈亏涨跌色」）。
 */

/** 触发器：help 光标提示「此处有解释」（触控轴无光标语义，挂了无害） */
export const trigger = style({
  cursor: 'help',
})

/** 角标「*」：小一号上标、随数值着色（sub/sup 默认 vertical-align: super 已就位，只收字号） */
export const marker = style({
  fontSize: '0.75em',
  lineHeight: 0,
})
