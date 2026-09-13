import { globalStyle } from '@vanilla-extract/css'

/**
 * 同步卡片旁路样式（ADR-0093 样式方案）：与组件同目录共置。
 *
 * 只承载本组件新增标记的少量字号收口——组件库元素（NText）没有字号 prop，
 * 卡片内的弱化提示一律 12px，钩子类 + globalStyle 不依赖样式注入顺序
 * （instrument-link.css.ts 先例）。存量内联样式随触碰渐进迁移，本票不批量改。
 */

/** 弱化提示小字钩子类（厂商预设说明、常用地域标签）。 */
export const SYNC_HINT_CLASS = 'sync-hint'

globalStyle(`.${SYNC_HINT_CLASS}`, {
  fontSize: '12px',
})
