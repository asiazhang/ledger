import { style } from '@vanilla-extract/css'
import { sprinkles } from '@ledger/theme/sprinkles.css.ts'

/**
 * 价格历史后台补全静默计数样式（issue #1375 / ADR-0122，ADR-0093 样式方案）：
 * 组件旁 vanilla-extract 样式文件（先例：sync-progress-bar.css.ts）。
 *
 * 静默语义的样式表达：弱化文字（低不透明度、小字号）、无背景无边界、
 * pointer-events 关闭（不可点、不拦截任何交互）、不占文档流外层（无弹层、
 * 无全局忙碌条）。不用蓝色进度条形态——那是手动同步的专属形态，后台补全
 * 只有一行弱化计数。
 */
export const root = style([
  sprinkles({
    display: 'flex',
    flexDirection: 'column',
  }),
  {
    gap: '2px',
    pointerEvents: 'none',
    userSelect: 'none',
  },
])

export const text = style({
  fontSize: '12px',
  lineHeight: 1.4,
  opacity: 0.6,
  whiteSpace: 'nowrap',
})
